//! skills.sh: one skill page, the repository behind it, and the dated audits it carries (D93).
//!
//! Two rules decide everything here:
//!
//! * **An audit is an input, not a grade.** The verdict is stored as evidence with its own
//!   date and URL; Rigel's static grade is untouched, so a directory's "Pass" can never lift
//!   a body Rigel has not read.
//! * **The page is parsed, never assumed.** If the markup moves, the importer says so and
//!   writes nothing rather than inventing a verdict. The audits are matched by their
//!   `/security/<vendor>` links, which is also the page the date comes from.
//!
//! Importing means indexing the skill's repository through the normal GitHub path, so the
//! body in the index is the repository's bytes, not a rendering of the page. The page only
//! supplies what GitHub cannot: the verdicts and their dates.

use regex::Regex;
use serde::Serialize;

use super::evidence::Evidence;
use super::http::Http;
use super::index::{IngestReport, SkillIndex};
use super::source::SyncPolicy;
use crate::migrate::now_ms;
use crate::skills::{SkillsError, DEFAULT_TTL};

/// The directory's canonical host. Importers accept any origin so a fixture can stand in,
/// but the prose and the tests use this one.
pub const SKILLS_SH_BASE: &str = "https://www.skills.sh";

/// `<a href="…/security/socket"><div …><span …>Socket</span><span …>Pass</span>`
const AUDIT_LINK: &str = r#"href="(?P<href>[^"]+/security/[a-z0-9-]+)"><div[^>]*><span[^>]*>(?P<vendor>[^<]+)</span><span[^>]*>(?P<verdict>[^<]+)</span>"#;

/// `First Seen</span></div><div …>Jan 26, 2026</div>`
const FIRST_SEEN: &str = r#"First Seen</span></div><div[^>]*>(?P<date>[^<]+)</div>"#;

/// `Audited by<!-- --> <span …>Socket</span> on<!-- --> <!-- -->Sep 15, 2026`
const AUDITED_ON: &str =
    r#"Audited by.{0,160}?on(?:<!-- -->|\s)*(?P<date>[A-Z][a-z]{2} \d{1,2}, \d{4})"#;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// One vendor's verdict, with the page that carries its date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditVerdict {
    /// The vendor as the page names it: `Gen Agent Trust Hub`, `Socket`, `Snyk`.
    pub vendor: String,
    /// Lowercased verdict: `pass`, `warn`, `fail`, or whatever the page said.
    pub verdict: String,
    /// The audit's own page. The date lives there, not on the skill page.
    pub url: String,
    /// `Sep 15, 2026`, as written. `None` when the audit page could not be read.
    pub audited_on: Option<String>,
    /// [`AuditVerdict::audited_on`] as epoch milliseconds, UTC midnight.
    pub audited_at: Option<i64>,
}

/// What a skills.sh skill page says, once parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsShPage {
    pub url: String,
    /// `owner/repo`, from the path.
    pub repo: String,
    /// The skill's slug, which is the page's last path segment.
    pub slug: String,
    pub first_seen_at: Option<i64>,
    pub audits: Vec<AuditVerdict>,
}

/// One `rigel skills import --skills-sh <url>`.
#[derive(Debug, Clone, Serialize)]
pub struct SkillsShReport {
    pub url: String,
    pub repo: String,
    /// The row the audits were attached to. `None` when the repository no longer carries a
    /// skill by that slug — the verdicts are then parsed and reported but not stored.
    pub skill_id: Option<String>,
    pub audits: Vec<AuditVerdict>,
    pub evidence: usize,
    pub indexed: IngestReport,
}

/// Parse a skill page. Pure: no network, no database, so the shape is pinned by tests.
pub fn parse_page(url: &str, html: &str) -> Result<SkillsShPage, SkillsError> {
    let (repo, slug, origin) = split_url(url)?;

    if !html.contains("Security Audits") && !html.contains("/security/") {
        return Err(SkillsError::Other(format!(
            "{url} does not look like a skills.sh skill page (no security audits section)"
        )));
    }

    let audit_re = Regex::new(AUDIT_LINK).expect("static audit pattern");
    let mut audits: Vec<AuditVerdict> = Vec::new();
    for caps in audit_re.captures_iter(html) {
        let href = caps["href"].to_string();
        let absolute = if href.starts_with("http://") || href.starts_with("https://") {
            href
        } else {
            format!("{origin}{href}")
        };
        if audits.iter().any(|known| known.url == absolute) {
            continue;
        }
        audits.push(AuditVerdict {
            vendor: caps["vendor"].trim().to_string(),
            verdict: caps["verdict"].trim().to_ascii_lowercase(),
            url: absolute,
            audited_on: None,
            audited_at: None,
        });
    }

    let first_seen_at = Regex::new(FIRST_SEEN)
        .expect("static first-seen pattern")
        .captures(html)
        .and_then(|caps| parse_date_ms(&caps["date"]));

    Ok(SkillsShPage {
        url: url.to_string(),
        repo,
        slug,
        first_seen_at,
        audits,
    })
}

/// Read the date off one audit page. `None` when the page changed shape — never "today".
pub fn parse_audit_date(html: &str) -> Option<String> {
    let caps = Regex::new(AUDITED_ON)
        .expect("static audit-date pattern")
        .captures(html)?;
    let text = caps["date"].trim().to_string();
    parse_date_ms(&text).map(|_| text)
}

/// Index the repository behind a skills.sh page and attach its dated audits.
pub async fn import(
    index: &SkillIndex,
    url: &str,
    policy: SyncPolicy,
) -> Result<SkillsShReport, SkillsError> {
    let http = Http::new(
        index.store().pool().clone(),
        index.blobs().clone(),
        DEFAULT_TTL,
    )?;
    let fetched = http.get_text(url).await?;
    let mut page = parse_page(url, &fetched.text)?;

    // One request per vendor. A failure costs the date, not the import.
    for audit in &mut page.audits {
        if let Ok(detail) = http.get_text(&audit.url).await {
            audit.audited_on = parse_audit_date(&detail.text);
            audit.audited_at = audit.audited_on.as_deref().and_then(parse_date_ms);
        }
    }

    let indexed = index.index_skill(&page.repo, policy, &page.slug).await?;
    let skill_id = find_skill(index, &page).await?;
    let evidence = attach(index, &page, skill_id.as_deref()).await?;

    Ok(SkillsShReport {
        url: page.url,
        repo: page.repo,
        skill_id,
        audits: page.audits,
        evidence,
        indexed,
    })
}

/// Write the verdicts against one row. `(skill_id, metric, source)` is the evidence key, so
/// a second import updates the date instead of adding a row.
pub async fn attach(
    index: &SkillIndex,
    page: &SkillsShPage,
    skill_id: Option<&str>,
) -> Result<usize, SkillsError> {
    let Some(skill_id) = skill_id else {
        return Ok(0);
    };
    let observed = now_ms();
    for audit in &page.audits {
        let evidence = Evidence {
            skill_id: skill_id.to_string(),
            metric: format!("audit:{}={}", audit_key(&audit.vendor), audit.verdict),
            measured: None,
            advertised: None,
            source: "skills.sh".to_string(),
            source_url: Some(audit.url.clone()),
            observed_at: audit.audited_at.unwrap_or(observed),
        };
        evidence.record(index.store().pool()).await?;
    }
    Ok(page.audits.len())
}

/// Find the indexed row this page describes: same repository, and either the same name or a
/// path ending in `/<slug>/SKILL.md`.
async fn find_skill(
    index: &SkillIndex,
    page: &SkillsShPage,
) -> Result<Option<String>, SkillsError> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM skills
          WHERE repo = ?
            AND (lower(name) = lower(?) OR path LIKE ?)
          ORDER BY (lower(name) = lower(?)) DESC, path
          LIMIT 1",
    )
    .bind(&page.repo)
    .bind(&page.slug)
    .bind(format!("%/{}/SKILL.md", page.slug))
    .bind(&page.slug)
    .fetch_optional(index.store().pool())
    .await?;
    Ok(id)
}

/// A stable, readable metric key: `gen`, `socket`, `snyk`, or the vendor slugged.
fn audit_key(vendor: &str) -> String {
    let lower = vendor.to_ascii_lowercase();
    if lower.contains("socket") {
        return "socket".to_string();
    }
    if lower.contains("snyk") {
        return "snyk".to_string();
    }
    if lower.contains("trust") || lower.contains("gen ") || lower == "gen" {
        return "gen".to_string();
    }
    lower
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// `https://www.skills.sh/owner/repo/slug` → (`owner/repo`, `slug`, origin).
fn split_url(url: &str) -> Result<(String, String, String), SkillsError> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest)
    } else {
        return Err(SkillsError::Other(format!(
            "{url} is not an http(s) skills.sh URL"
        )));
    };

    let (host, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if host.is_empty() {
        return Err(SkillsError::Other(format!("{url} has no host")));
    }

    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if segments.len() < 3 {
        return Err(SkillsError::Other(format!(
            "{url} is not a skill page — expected /<owner>/<repo>/<skill>"
        )));
    }
    if segments.len() >= 4 && segments[3] == "security" {
        return Err(SkillsError::Other(format!(
            "{url} is an audit page; pass the skill page instead"
        )));
    }

    Ok((
        format!("{}/{}", segments[0], segments[1]),
        segments[2].to_string(),
        format!("{scheme}://{host}"),
    ))
}

/// `Jan 26, 2026` → epoch milliseconds at UTC midnight.
pub fn parse_date_ms(text: &str) -> Option<i64> {
    let text = text.trim();
    let (month, rest) = text.split_once(' ')?;
    let (day, year) = rest.split_once(',')?;
    let day: i64 = day.trim().parse().ok()?;
    let year: i64 = year.trim().parse().ok()?;
    let month = MONTHS.iter().position(|name| *name == month)? as i64 + 1;
    if !(1..=31).contains(&day) || !(1970..=2100).contains(&year) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400_000)
}

/// Howard Hinnant's days-from-civil, so the crate needs no date dependency.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
