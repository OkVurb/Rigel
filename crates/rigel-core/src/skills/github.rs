//! GitHub as a skill source: `owner/repo`, or any GitHub URL pasted at the user.
//!
//! Cost model (from `docs/skill-intelligence.md`): repo metadata and one recursive tree call
//! per repo, then `raw.githubusercontent.com` for bodies, which does not touch the API quota.
//! That is 2 API calls for a 300-skill repository instead of 300.
//!
//! The tree response is capped by GitHub at 100,000 entries and says so with `truncated`;
//! rather than silently indexing part of a huge monorepo, the snapshot carries the flag and
//! the CLI prints it.

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use sqlx::SqlitePool;

use super::blob::{sha256_hex, BlobStore};
use super::frontmatter;
use super::http::Http;
use super::scan;
use super::source::{FetchedSkill, RepoSnapshot, SkillRecord, Source, SourceStatus, SyncPolicy};
use crate::migrate::now_ms;
use crate::skills::{cluster, fmt_time, SkillsError};

const API: &str = "https://api.github.com";
const RAW: &str = "https://raw.githubusercontent.com";

/// A guard against a pathological tree, not a product limit: ECC's 311 skills fit
/// comfortably. `--limit` raises it.
pub const DEFAULT_REPO_LIMIT: usize = 500;

/// Bodies are fetched a few at a time. Three hundred simultaneous connections to
/// raw.githubusercontent.com is how a polite client becomes a blocked one.
const CONCURRENCY: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub repo: String,
    pub git_ref: Option<String>,
    pub path: Option<String>,
}

/// Ported from the prototype, including the "a pasted file URL collapses to its directory"
/// behaviour — pasting a `SKILL.md` link is the single most common way this input arrives.
pub fn parse_repo_input(raw: &str) -> Option<RepoRef> {
    let input = raw.trim();
    let input = input.strip_suffix(".git").unwrap_or(input);
    let input = input.trim_end_matches('/');
    if input.is_empty() {
        return None;
    }

    if !input.starts_with("http://") && !input.starts_with("https://") {
        let (head, rest) = input.split_once('/')?;
        if head.is_empty() || rest.is_empty() {
            return None;
        }
        let segments: Vec<&str> = rest.split('/').collect();
        let repo = segments[0];
        if repo.is_empty() {
            return None;
        }
        // owner/repo/tree/<ref>/<path> and owner/repo/blob/<ref>/<path>
        if segments.len() >= 4 && (segments[1] == "tree" || segments[1] == "blob") {
            let git_ref = segments[2];
            let path = segments[3..].join("/");
            let path = if segments[1] == "blob" {
                parent_dir(&path)
            } else {
                path
            };
            return Some(RepoRef {
                owner: head.to_string(),
                repo: repo.to_string(),
                git_ref: Some(git_ref.to_string()),
                path: non_empty(path),
            });
        }
        if segments.len() > 1 {
            return Some(RepoRef {
                owner: head.to_string(),
                repo: repo.to_string(),
                git_ref: None,
                path: non_empty(segments[1..].join("/")),
            });
        }
        return Some(RepoRef {
            owner: head.to_string(),
            repo: repo.to_string(),
            git_ref: None,
            path: None,
        });
    }

    let without_scheme = input.split_once("://")?.1;
    let (host, rest) = without_scheme.split_once('/')?;
    if !(host == "github.com" || host.ends_with(".github.com")) {
        return None;
    }
    let segments: Vec<&str> = rest.trim_matches('/').split('/').collect();
    if segments.len() < 2 {
        return None;
    }
    let (owner, repo) = (segments[0], segments[1]);

    if segments.len() >= 5 && (segments[2] == "tree" || segments[2] == "blob") {
        let git_ref = segments[3];
        let mut path = segments[4..].join("/");
        if segments[2] == "blob" || path.to_ascii_lowercase().ends_with(".md") {
            path = parent_dir(&path);
        }
        return Some(RepoRef {
            owner: owner.to_string(),
            repo: repo.to_string(),
            git_ref: Some(git_ref.to_string()),
            path: non_empty(path),
        });
    }

    Some(RepoRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        git_ref: None,
        path: None,
    })
}

fn parent_dir(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

pub struct GitHubSource {
    http: Http,
    pool: SqlitePool,
    limit: usize,
    /// When set, only the skill whose directory is `<slug>` is kept. The skills.sh import
    /// uses it so one page costs one body rather than a whole repository.
    slug: Option<String>,
    api: String,
    raw: String,
}

impl GitHubSource {
    pub async fn new(
        pool: SqlitePool,
        blobs: BlobStore,
        ttl: Duration,
    ) -> Result<Self, SkillsError> {
        Ok(Self {
            http: Http::new(pool.clone(), blobs, ttl)?,
            pool,
            limit: DEFAULT_REPO_LIMIT,
            slug: None,
            api: API.to_string(),
            raw: RAW.to_string(),
        })
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    /// Keep only the skill at `<slug>/SKILL.md` or `…/<slug>/SKILL.md`.
    pub fn with_slug(mut self, slug: &str) -> Self {
        self.slug = Some(slug.to_string());
        self
    }

    /// Point the source at a fixture instead of GitHub. Only tests use this; it exists
    /// because the ETag and freshness behaviour is too fragile to verify by hand.
    pub fn with_endpoints(mut self, api: &str, raw: &str) -> Self {
        self.api = api.trim_end_matches('/').to_string();
        self.raw = raw.trim_end_matches('/').to_string();
        self
    }

    pub fn endpoints(&self) -> (&str, &str) {
        (&self.api, &self.raw)
    }

    pub fn http(&self) -> &Http {
        &self.http
    }

    /// The tree walk, split out so tests can drive it against a fixture server.
    pub async fn walk(
        &self,
        target: &str,
        policy: SyncPolicy,
    ) -> Result<RepoSnapshot, SkillsError> {
        let reference = parse_repo_input(target).ok_or_else(|| {
            SkillsError::InvalidRepo(format!("{target:?} is not a GitHub repository or URL"))
        })?;

        let before = self.http.requests();

        let meta_url = format!("{}/repos/{}/{}", self.api, reference.owner, reference.repo);
        let meta = self.http.get_json(&meta_url, policy.force).await?;

        let branch = reference
            .git_ref
            .clone()
            .or_else(|| meta["default_branch"].as_str().map(ToOwned::to_owned))
            .ok_or_else(|| {
                SkillsError::Other(format!(
                    "{} has no default branch — is it empty?",
                    reference.repo
                ))
            })?;

        let tree_url = format!(
            "{}/repos/{}/{}/git/trees/{}?recursive=1",
            self.api, reference.owner, reference.repo, branch
        );
        let tree = self.http.get_json(&tree_url, policy.force).await?;

        let entries = tree["tree"].as_array().cloned().unwrap_or_default();
        let mut candidates: Vec<(String, i64)> = entries
            .iter()
            .filter(|entry| entry["type"].as_str() == Some("blob"))
            .filter_map(|entry| {
                let path = entry["path"].as_str()?;
                if path == "SKILL.md" || path.ends_with("/SKILL.md") {
                    Some((path.to_string(), entry["size"].as_i64().unwrap_or(0)))
                } else {
                    None
                }
            })
            .collect();

        if let Some(scope) = &reference.path {
            let scoped: Vec<(String, i64)> = candidates
                .iter()
                .filter(|(path, _)| path.starts_with(scope.as_str()))
                .cloned()
                .collect();
            if !scoped.is_empty() {
                candidates = scoped;
            }
        }

        if let Some(slug) = &self.slug {
            candidates.retain(|(path, _)| {
                path == &format!("{slug}/SKILL.md") || path.ends_with(&format!("/{slug}/SKILL.md"))
            });
            if candidates.is_empty() {
                return Err(SkillsError::NoSkillsFound(format!(
                    "no SKILL.md under {slug}/ in {}/{} ({branch})",
                    reference.owner, reference.repo
                )));
            }
        }

        if candidates.is_empty() {
            return Err(SkillsError::NoSkillsFound(format!(
                "no SKILL.md in {}/{} ({branch})",
                reference.owner, reference.repo
            )));
        }

        let found = candidates.len();
        candidates.truncate(self.limit);

        // Owned jobs, not borrowed ones: every closure here has to satisfy the boxed-future
        // lifetime that `async_trait` requires, and borrowing `path` from the tree is what
        // makes that impossible.
        let stars = meta["stargazers_count"].as_i64().unwrap_or(0);
        let jobs: Vec<(String, String, String, String, String, i64)> = candidates
            .iter()
            .map(|(path, _)| {
                (
                    format!(
                        "{}/{}/{}/{}/{}",
                        self.raw,
                        reference.owner,
                        reference.repo,
                        branch,
                        encode_path(path)
                    ),
                    reference.owner.clone(),
                    reference.repo.clone(),
                    branch.clone(),
                    path.clone(),
                    stars,
                )
            })
            .collect();

        let fetched: Vec<Option<FetchedSkill>> = futures::stream::iter(jobs)
            .map(|(raw_url, owner, repo, branch, path, stars)| {
                let http = self.http.clone();
                let force = policy.force;
                async move {
                    let fetched = match http.get_text_with(&raw_url, "text/plain", force).await {
                        Ok(fetched) => fetched,
                        Err(error) => {
                            // One unreadable body must not sink the whole repo: the other
                            // 299 skills are still worth indexing.
                            eprintln!("rigel: skipping {path}: {error}");
                            return None;
                        }
                    };
                    Some(build_skill(
                        &fetched.text,
                        &owner,
                        &repo,
                        &branch,
                        &path,
                        stars,
                    ))
                }
            })
            .buffer_unordered(CONCURRENCY)
            .collect()
            .await;

        let skills: Vec<FetchedSkill> = fetched.into_iter().flatten().collect();
        if skills.is_empty() {
            return Err(SkillsError::NoSkillsFound(format!(
                "found {found} SKILL.md files in {}/{} but could not load any",
                reference.owner, reference.repo
            )));
        }

        let mut sorted = skills;
        sorted.sort_by(|a, b| a.record.id.cmp(&b.record.id));

        Ok(RepoSnapshot {
            source: "github".to_string(),
            repo: meta["full_name"]
                .as_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{}/{}", reference.owner, reference.repo)),
            branch,
            stars,
            license: meta["license"]["spdx_id"].as_str().map(ToOwned::to_owned),
            truncated: tree["truncated"].as_bool().unwrap_or(false),
            found,
            http_requests: self.http.requests().saturating_sub(before),
            skills: sorted,
        })
    }
}

fn build_skill(
    raw: &str,
    owner: &str,
    repo: &str,
    branch: &str,
    path: &str,
    stars: i64,
) -> FetchedSkill {
    let parsed = frontmatter::parse(raw);
    let name = parsed
        .get("name")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let dir = parent_dir(path);
            let last = dir.rsplit('/').next().unwrap_or_default().to_string();
            if last.is_empty() {
                None
            } else {
                Some(last)
            }
        })
        .unwrap_or_else(|| repo.to_string());

    let description = parsed
        .get("description")
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .unwrap_or("No description in frontmatter.")
        .chars()
        .take(240)
        .collect::<String>();

    let capabilities = scan::scan(&parsed.body, path);
    let grade = scan::grade(&capabilities);
    let tokens = SkillRecord::estimate_tokens(&parsed.body);
    let fingerprint = cluster::simhash(&parsed.body);

    let blob_path = encode_path(path);
    FetchedSkill {
        record: SkillRecord {
            id: format!("github:{owner}/{repo}:{path}"),
            name,
            description,
            source: "github".to_string(),
            repo: Some(format!("{owner}/{repo}")),
            path: Some(path.to_string()),
            url: Some(format!(
                "https://github.com/{owner}/{repo}/blob/{branch}/{blob_path}"
            )),
            body_sha: Some(sha256_hex(raw.as_bytes())),
            grade: Some(grade.to_string()),
            grade_scope: Some(scan::grade_scope().to_string()),
            license: parsed.get("license").map(ToOwned::to_owned),
            stars: Some(stars),
            installs: None,
            tokens: Some(tokens),
            capabilities,
            simhash: Some(fingerprint as i64),
        },
        body: raw.to_string(),
    }
}

#[async_trait]
impl Source for GitHubSource {
    fn id(&self) -> &'static str {
        "github"
    }

    async fn status(&self) -> Result<SourceStatus, SkillsError> {
        Ok(super::source::load_status(&self.pool, "github", Some(&self.api)).await?)
    }

    async fn fetch_repo(
        &self,
        target: &str,
        policy: SyncPolicy,
    ) -> Result<RepoSnapshot, SkillsError> {
        self.walk(target, policy).await
    }
}

/// Human summary used by the CLI and the Tauri command.
pub fn describe(snapshot: &RepoSnapshot) -> String {
    let when = fmt_time(now_ms());
    format!(
        "{} skills from {}@{} · found {} · {} requests · {}",
        snapshot.skills.len(),
        snapshot.repo,
        snapshot.branch,
        snapshot.found,
        snapshot.http_requests,
        when
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_and_url_forms_parse_the_same_way() {
        let expected = RepoRef {
            owner: "acme".into(),
            repo: "skills".into(),
            git_ref: None,
            path: None,
        };
        for input in [
            "acme/skills",
            "https://github.com/acme/skills",
            "https://github.com/acme/skills/",
            "https://github.com/acme/skills.git",
        ] {
            assert_eq!(parse_repo_input(input), Some(expected.clone()), "{input}");
        }
    }

    #[test]
    fn a_pasted_skill_url_collapses_to_its_directory() {
        assert_eq!(
            parse_repo_input("https://github.com/acme/skills/blob/main/pdf/SKILL.md"),
            Some(RepoRef {
                owner: "acme".into(),
                repo: "skills".into(),
                git_ref: Some("main".into()),
                path: Some("pdf".into()),
            })
        );
        assert_eq!(
            parse_repo_input("https://github.com/acme/skills/tree/main/pdf"),
            Some(RepoRef {
                owner: "acme".into(),
                repo: "skills".into(),
                git_ref: Some("main".into()),
                path: Some("pdf".into()),
            })
        );
    }

    #[test]
    fn junk_is_rejected_rather_than_guessed_at() {
        for input in ["", "   ", "https://gitlab.com/acme/skills", "noslash"] {
            assert_eq!(parse_repo_input(input), None, "{input}");
        }
    }

    #[test]
    fn paths_are_encoded_for_the_raw_host() {
        assert_eq!(encode_path("pdf tools/SKILL.md"), "pdf%20tools/SKILL.md");
        assert_eq!(encode_path("a/b/SKILL.md"), "a/b/SKILL.md");
    }

    #[test]
    fn a_parsed_skill_carries_grade_evidence_and_fingerprint() {
        let raw = "---\nname: \"pdf reader\"\ndescription: Read scanned PDFs.\nlicense: MIT\n---\n# PDF\nRun `curl https://x.dev` to fetch the page.\n";
        let skill = build_skill(raw, "acme", "skills", "main", "pdf/SKILL.md", 42);

        assert_eq!(skill.record.id, "github:acme/skills:pdf/SKILL.md");
        assert_eq!(skill.record.name, "pdf reader");
        assert_eq!(skill.record.license.as_deref(), Some("MIT"));
        assert_eq!(skill.record.grade.as_deref(), Some("C"));
        assert_eq!(
            skill.record.simhash,
            Some(cluster::simhash(&frontmatter::parse(raw).body) as i64)
        );
        assert!(skill.record.body_sha.is_some());
        assert!(skill
            .record
            .url
            .as_deref()
            .unwrap()
            .ends_with("/pdf/SKILL.md"));
        assert_eq!(skill.record.capabilities.len(), 1, "network access only");
    }
}
