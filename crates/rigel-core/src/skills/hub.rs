//! The two marketplaces, moved out of the prototype's dev proxy and into the indexer.
//!
//! * **SkillsMP** answers JSON and points at a GitHub directory. Rigel indexes the metadata
//!   from the marketplace row and scans the body when the GitHub source walks that repo —
//!   the marketplace is a discovery surface, not a body store.
//! * **SkillHub** answers HTML only. There is no API, so the list page is parsed for cards
//!   and the detail page for the SKILL.md body it renders. If either page changes shape the
//!   connector returns no rows rather than inventing them, and the source goes `broken`.
//!
//! Both report through [`SourceStatus`]: a marketplace that was reachable yesterday is
//! `stale` with its last sync date, not hidden.

use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use sqlx::SqlitePool;

use super::blob::{BlobStore, sha256_hex};
use super::frontmatter;
use super::http::Http;
use super::scan;
use super::source::{FetchedSkill, SkillRecord, Source, SourceStatus};
use crate::skills::{SkillsError, cluster};

const SKILLSMP_BASE: &str = "https://skillsmp.com";
const SKILLHUB_BASE: &str = "https://skills.browseract.com";

const SKILLSMP_ID: &str = "skillsmp";
const SKILLHUB_ID: &str = "skillhub";

pub struct SkillsMpSource {
    http: Http,
    pool: SqlitePool,
}

impl SkillsMpSource {
    pub async fn new(
        pool: SqlitePool,
        blobs: BlobStore,
        ttl: Duration,
    ) -> Result<Self, SkillsError> {
        Ok(Self {
            http: Http::new(pool.clone(), blobs, ttl)?,
            pool,
        })
    }

    pub async fn search_records(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SkillRecord>, SkillsError> {
        self.search_records_page(query, limit as u32, 1).await
    }

    /// SkillsMP accepts `limit` in `1..=50` and pages results with `page=<n>` (verified
    /// 21 Oct 2026: page 2 returns 50 rows with zero ids overlapping page 1). A page beyond
    /// the end returns an empty `skills` array, which the sweep uses as the stop signal.
    pub async fn search_records_page(
        &self,
        query: &str,
        limit: u32,
        page: u32,
    ) -> Result<Vec<SkillRecord>, SkillsError> {
        let url = format!(
            "{SKILLSMP_BASE}/api/v1/skills/search?q={}&limit={}&sortBy=stars&page={}",
            encode_query(query),
            limit.clamp(1, 50),
            page
        );
        let body = self.http.get_json(&url, false).await?;
        Ok(parse_skillsmp(&body))
    }
}

#[async_trait]
impl Source for SkillsMpSource {
    fn id(&self) -> &'static str {
        SKILLSMP_ID
    }

    async fn status(&self) -> Result<SourceStatus, SkillsError> {
        Ok(super::source::load_status(&self.pool, SKILLSMP_ID, Some(SKILLSMP_BASE)).await?)
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SkillRecord>, SkillsError> {
        self.search_records(query, limit).await
    }
}

pub struct SkillHubSource {
    http: Http,
    pool: SqlitePool,
}

impl SkillHubSource {
    pub async fn new(
        pool: SqlitePool,
        blobs: BlobStore,
        ttl: Duration,
    ) -> Result<Self, SkillsError> {
        Ok(Self {
            http: Http::new(pool.clone(), blobs, ttl)?,
            pool,
        })
    }

    pub async fn browse(&self, page: u32, limit: u32) -> Result<Vec<SkillRecord>, SkillsError> {
        let url = format!("{SKILLHUB_BASE}/skills?page={page}&limit={limit}&sort=safest");
        let fetched = self.http.get_text_with(&url, "text/html", false).await?;
        Ok(parse_skillhub_cards(&fetched.text))
    }

    pub async fn load(&self, slug: &str) -> Result<FetchedSkill, SkillsError> {
        if !is_slug(slug) {
            return Err(SkillsError::NotIndexed(slug.to_string()));
        }

        let url = format!("{SKILLHUB_BASE}/skills/{slug}");
        let fetched = self.http.get_text_with(&url, "text/html", false).await?;
        let page = parse_skillhub_detail(&fetched.text);
        let body = page.body;
        if body.trim().is_empty() {
            return Err(SkillsError::NoBody(format!("skillhub:{slug}")));
        }

        let parsed = frontmatter::parse(&body);
        let name = parsed
            .get("name")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&page.name)
            .to_string();
        let description = parsed
            .get("description")
            .map(str::trim)
            .filter(|description| !description.is_empty())
            .or(page.description.as_deref())
            .unwrap_or("No description in frontmatter.")
            .chars()
            .take(240)
            .collect::<String>();

        let capabilities = scan::scan(&parsed.body, &format!("{slug}/SKILL.md"));
        let grade = scan::grade(&capabilities);

        Ok(FetchedSkill {
            record: SkillRecord {
                id: format!("skillhub:{slug}"),
                name,
                description,
                source: SKILLHUB_ID.to_string(),
                repo: page.repo.clone(),
                path: Some(format!("{slug}/SKILL.md")),
                url: Some(url),
                body_sha: Some(sha256_hex(body.as_bytes())),
                grade: Some(grade.to_string()),
                grade_scope: Some(scan::grade_scope().to_string()),
                license: None,
                stars: None,
                installs: page.installs,
                tokens: Some(SkillRecord::estimate_tokens(&parsed.body)),
                capabilities,
                simhash: Some(cluster::simhash(&parsed.body) as i64),
            },
            body,
        })
    }
}

#[async_trait]
impl Source for SkillHubSource {
    fn id(&self) -> &'static str {
        SKILLHUB_ID
    }

    async fn status(&self) -> Result<SourceStatus, SkillsError> {
        Ok(super::source::load_status(&self.pool, SKILLHUB_ID, Some(SKILLHUB_BASE)).await?)
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SkillRecord>, SkillsError> {
        // SkillHub has no search endpoint; keep the last browse page and filter it locally.
        let records = self.browse(1, 36).await?;
        let needle = query.trim().to_ascii_lowercase();
        Ok(records
            .into_iter()
            .filter(|record| {
                needle.is_empty()
                    || record.name.to_ascii_lowercase().contains(&needle)
                    || record.description.to_ascii_lowercase().contains(&needle)
            })
            .take(limit)
            .collect())
    }

    async fn fetch_skill(&self, id: &str) -> Result<FetchedSkill, SkillsError> {
        let slug = id
            .strip_prefix("skillhub:")
            .ok_or_else(|| SkillsError::NotIndexed(id.to_string()))?;
        self.load(slug).await
    }
}

/// The JSON shape SkillsMP returns, as the prototype read it: `{ data: { skills: [...] } }`.
fn parse_skillsmp(body: &Value) -> Vec<SkillRecord> {
    let skills = body["data"]["skills"]
        .as_array()
        .or_else(|| body["skills"].as_array());

    let Some(skills) = skills else {
        return Vec::new();
    };

    skills
        .iter()
        .filter_map(|entry| {
            let id = entry["id"].as_str().or_else(|| entry["slug"].as_str())?;
            let name = entry["name"].as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            Some(SkillRecord {
                id: format!("skillsmp:{id}"),
                name: name.to_string(),
                description: entry["description"]
                    .as_str()
                    .unwrap_or("No description in frontmatter.")
                    .chars()
                    .take(240)
                    .collect(),
                source: SKILLSMP_ID.to_string(),
                repo: entry["githubUrl"].as_str().and_then(github_repo_from_url),
                path: None,
                url: entry["skillUrl"]
                    .as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(format!("{SKILLSMP_BASE}/skills/{id}"))),
                body_sha: None,
                // NULL grade means "not scanned", which the shelf renders as such. A
                // marketplace listing is not a review.
                grade: None,
                grade_scope: None,
                license: None,
                stars: entry["stars"].as_i64(),
                installs: entry["installs"].as_i64(),
                tokens: None,
                capabilities: Vec::new(),
                simhash: None,
            })
        })
        .collect()
}

fn github_repo_from_url(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let mut segments = rest.split('/');
    let owner = segments.next()?;
    let repo = segments.next()?.trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

/// Cards from the SkillHub list page. Every field is optional except name and slug: a card we
/// cannot identify is skipped, a card with no stats keeps its row.
pub fn parse_skillhub_cards(html: &str) -> Vec<SkillRecord> {
    let card_re =
        Regex::new(r#"(?s)<a class="skill-card[^"]*" href="/skills/([^"?]+)[^"]*">([\s\S]*?)</a>"#)
            .expect("card regex compiles");
    let name_re = Regex::new(r#"class="sc-name">([^<]+)<"#).unwrap();
    let desc_re = Regex::new(r#"(?s)class="skill-desc sc-desc">([\s\S]*?)</p>"#).unwrap();
    let installs_re = Regex::new(r#"(?s)title="Installs"[\s\S]*?</svg>\s*([^<]+)"#).unwrap();
    let stars_re = Regex::new(r#"(?s)title="GitHub stars"[\s\S]*?</svg>\s*([^<]+)"#).unwrap();

    let mut records = Vec::new();
    for capture in card_re.captures_iter(html) {
        let slug = capture[1].trim();
        let body = &capture[2];
        let Some(name) = name_re.captures(body).map(|c| decode(&c[1])) else {
            continue;
        };
        if name.trim().is_empty() || !is_slug(slug) {
            continue;
        }
        let description = desc_re
            .captures(body)
            .map(|c| decode(&c[1]))
            .unwrap_or_default();

        records.push(SkillRecord {
            id: format!("skillhub:{slug}"),
            name: name.trim().to_string(),
            description: description.chars().take(240).collect(),
            source: SKILLHUB_ID.to_string(),
            repo: None,
            path: Some(format!("{slug}/SKILL.md")),
            url: Some(format!("{SKILLHUB_BASE}/skills/{slug}")),
            body_sha: None,
            grade: None,
            grade_scope: None,
            license: None,
            stars: stars_re.captures(body).and_then(|c| parse_count(&c[1])),
            installs: installs_re.captures(body).and_then(|c| parse_count(&c[1])),
            tokens: None,
            capabilities: Vec::new(),
            simhash: None,
        });
    }
    records
}

struct HubDetail {
    name: String,
    description: Option<String>,
    repo: Option<String>,
    installs: Option<i64>,
    body: String,
}

fn parse_skillhub_detail(html: &str) -> HubDetail {
    let article_re =
        Regex::new(r#"(?s)<article class="sd-ext-skillmd">([\s\S]*?)</article>"#).unwrap();
    let title_re = Regex::new(r#"(?s)<h1[^>]*>([\s\S]*?)</h1>"#).unwrap();
    let meta_re = Regex::new(r#"(?s)<meta name="description" content="([^"]*)""#).unwrap();
    let repo_re = Regex::new(r#"https://github\.com/([\w.-]+/[\w.-]+)"#).unwrap();

    let body = article_re
        .captures(html)
        .map(|c| html_to_text(&c[1]))
        .unwrap_or_default();

    HubDetail {
        name: title_re
            .captures(html)
            .map(|c| decode(&html_to_text(&c[1])))
            .unwrap_or_default(),
        description: meta_re.captures(html).map(|c| decode(&c[1])),
        repo: repo_re.captures(html).map(|c| c[1].to_string()),
        installs: None,
        body,
    }
}

/// HTML → readable text, ported from the prototype's proxy so the bodies the index stores
/// match what the prototype showed.
pub fn html_to_text(html: &str) -> String {
    let block_re = Regex::new(r"(?i)</(p|div|li|h[1-6]|tr)>").unwrap();
    let br_re = Regex::new(r"(?i)<br\s*/?>").unwrap();
    let li_re = Regex::new(r"(?i)<li[^>]*>").unwrap();
    let h_re = Regex::new(r"(?i)<h[1-6][^>]*>").unwrap();
    let code_re = Regex::new(r"(?is)<code[^>]*>([\s\S]*?)</code>").unwrap();
    let blank_runs = Regex::new(r"\n{3,}").unwrap();

    let text = block_re.replace_all(html, "\n").into_owned();
    let text = br_re.replace_all(&text, "\n").into_owned();
    let text = li_re.replace_all(&text, "- ").into_owned();
    let text = h_re.replace_all(&text, "\n## ").into_owned();
    let text = code_re.replace_all(&text, "`$1`").into_owned();
    let text = decode(&text);
    blank_runs.replace_all(&text, "\n\n").trim().to_string()
}

/// The entity set the prototype decoded. Anything else stays literal, which is better than
/// silently dropping it.
pub fn decode(text: &str) -> String {
    let mut out = text
        .replace("<!-- -->", "")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    if out.contains('<') {
        let tag_re = Regex::new(r"<[^>]+>").unwrap();
        out = tag_re.replace_all(&out, " ").into_owned();
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `1.2k`, `44,000`, `1.2M` → a number. Marketplace stats are strings because they are
/// formatted for humans.
pub fn parse_count(raw: &str) -> Option<i64> {
    let cleaned = raw.trim().replace([',', ' '], "");
    if cleaned.is_empty() {
        return None;
    }
    let (number, multiplier) = match cleaned.chars().last()? {
        'k' | 'K' => (&cleaned[..cleaned.len() - 1], 1_000_f64),
        'm' | 'M' => (&cleaned[..cleaned.len() - 1], 1_000_000_f64),
        _ => (cleaned.as_str(), 1_f64),
    };
    let value: f64 = number.parse().ok()?;
    Some((value * multiplier).round() as i64)
}

fn is_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 120
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn encode_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skillsmp_rows_become_records_with_repo_provenance() {
        let body: Value = serde_json::from_str(
            r#"{"data":{"skills":[
                {"id":"s1","name":"pdf-tools","author":"acme","description":"Extract text",
                 "skillUrl":"https://skillsmp.com/skills/pdf-tools","stars":42,
                 "githubUrl":"https://github.com/acme/skills/tree/main/pdf"}
            ]}}"#,
        )
        .unwrap();

        let records = parse_skillsmp(&body);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "skillsmp:s1");
        assert_eq!(records[0].repo.as_deref(), Some("acme/skills"));
        assert_eq!(records[0].stars, Some(42));
        assert_eq!(records[0].grade, None, "a marketplace row is not a review");
    }

    #[test]
    fn skillhub_cards_survive_a_missing_stat_line() {
        let html = r#"
          <a class="skill-card featured" href="/skills/pdf-reader?ref=list">
            <span class="sc-name">pdf-reader</span>
            <p class="skill-desc sc-desc">Reads <code>PDF</code> files</p>
            <span class="sc-author">acme</span>
            <span title="Installs"><svg></svg> 1.2k</span>
          </a>
          <a class="skill-card" href="/skills/deploy">
            <span class="sc-name">deploy</span>
          </a>
          <a class="skill-card" href="/skills/bad slug"><span class="sc-name">bad</span></a>
        "#;

        let records = parse_skillhub_cards(html);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "skillhub:pdf-reader");
        assert_eq!(records[0].installs, Some(1200));
        assert_eq!(records[0].stars, None);
        assert_eq!(records[0].description, "Reads PDF files");
        assert_eq!(records[1].grade, None);
    }

    #[test]
    fn the_detail_page_yields_a_body_and_a_repo() {
        let html = r#"
          <h1>pdf-reader</h1>
          <meta name="description" content="Reads PDF files">
          <article class="sd-ext-skillmd">
            <h2>Use</h2><p>Ask the agent to read <code>report.pdf</code>.</p>
            <ul><li>Then summarise</li></ul>
          </article>
          <a href="https://github.com/acme/skills">source</a>
        "#;
        let detail = parse_skillhub_detail(html);
        assert_eq!(detail.name, "pdf-reader");
        assert_eq!(detail.repo.as_deref(), Some("acme/skills"));
        assert_eq!(detail.description.as_deref(), Some("Reads PDF files"));
        assert!(detail.body.contains("## Use"));
        assert!(detail.body.contains("- Then summarise"));
    }

    #[test]
    fn human_counts_parse_including_suffixes() {
        assert_eq!(parse_count("44,000"), Some(44_000));
        assert_eq!(parse_count("1.2k"), Some(1200));
        assert_eq!(parse_count("1.2M"), Some(1_200_000));
        assert_eq!(parse_count(""), None);
        assert_eq!(parse_count("lots"), None);
    }
}
