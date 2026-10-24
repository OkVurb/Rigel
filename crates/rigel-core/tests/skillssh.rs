//! D93: skills.sh pages become dated evidence, and the directory's verdict never becomes
//! Rigel's grade.
//!
//! The HTML below is a trimmed copy of a real page (22 Oct 2026), because the point of the
//! parser is the shape a vendor actually serves, not one invented to match the regex.

use rigel_core::skills::skillssh::{
    AuditVerdict, SkillsShPage, attach, parse_audit_date, parse_date_ms, parse_page,
};
use rigel_core::skills::{SkillIndex, SkillRecord};
use tempfile::TempDir;

const URL: &str = "https://www.skills.sh/vercel-labs/skills/find-skills";

const PAGE: &str = r#"<div class="bg-background py-8"><div class="text-sm font-mono uppercase text-white mb-2"><span>First Seen</span></div><div class="text-sm font-mono text-foreground">Jan 26, 2026</div></div><div class="bg-background py-8"><div class="text-sm font-mono uppercase text-white mb-3">Security Audits</div><div class="divide-y divide-border"><a class="block py-2 hover:bg-muted/40" href="/vercel-labs/skills/find-skills/security/agent-trust-hub"><div class="flex items-center justify-between mb-1"><span class="text-sm font-medium text-foreground truncate">Gen Agent Trust Hub</span><span class="text-xs font-mono uppercase px-2 py-1 rounded bg-green-500/10 text-green-500">Pass</span></div></a><a class="block py-2 hover:bg-muted/40" href="/vercel-labs/skills/find-skills/security/socket"><div class="flex items-center justify-between mb-1"><span class="text-sm font-medium text-foreground truncate">Socket</span><span class="text-xs font-mono uppercase px-2 py-1 rounded bg-green-500/10 text-green-500">Pass</span></div></a><a class="block py-2 hover:bg-muted/40" href="/vercel-labs/skills/find-skills/security/snyk"><div class="flex items-center justify-between mb-1"><span class="text-sm font-medium text-foreground truncate">Snyk</span><span class="text-xs font-mono uppercase px-2 py-1 rounded bg-amber-500/10 text-amber-500">Warn</span></div></a></div></div>"#;

const AUDIT: &str = r#"<div class="mb-10"><h1 class="text-4xl font-semibold text-foreground tracking-tight mb-2">find-skills</h1><div class="flex flex-wrap items-center gap-2"><span class="inline-block text-xs font-mono uppercase px-2 py-1 rounded bg-green-500/10 text-green-500">Pass</span><p class="text-sm text-(--ds-gray-600)">Audited by<!-- --> <span class="text-foreground">Socket</span> on<!-- --> <!-- -->Sep 15, 2026</p></div></div>"#;

#[test]
fn a_skill_page_names_its_repository_its_first_seen_and_every_verdict() {
    let page = parse_page(URL, PAGE).expect("a real skill page parses");

    assert_eq!(page.repo, "vercel-labs/skills");
    assert_eq!(page.slug, "find-skills");
    assert_eq!(page.first_seen_at, Some(1_769_385_600_000), "Jan 26, 2026");

    let named: Vec<(&str, &str)> = page
        .audits
        .iter()
        .map(|audit| (audit.vendor.as_str(), audit.verdict.as_str()))
        .collect();
    assert_eq!(
        named,
        vec![
            ("Gen Agent Trust Hub", "pass"),
            ("Socket", "pass"),
            ("Snyk", "warn"),
        ],
        "the three vendors, in page order, with their own verdicts"
    );
    assert_eq!(
        page.audits[1].url, "https://www.skills.sh/vercel-labs/skills/find-skills/security/socket",
        "relative security links resolve against the page's origin"
    );
    assert!(
        page.audits.iter().all(|audit| audit.audited_at.is_none()),
        "the skill page carries no dates; they live on the audit pages"
    );
}

#[test]
fn an_audit_page_carries_the_date_the_verdict_was_issued() {
    assert_eq!(
        parse_audit_date(AUDIT).as_deref(),
        Some("Sep 15, 2026"),
        "the date comes off the audit page, not the fetch clock"
    );
    assert_eq!(parse_date_ms("Sep 15, 2026"), Some(1_789_430_400_000));
    assert_eq!(parse_date_ms("Feb 28, 2026"), Some(1_772_236_800_000));
    assert_eq!(parse_date_ms("Smarch 1, 2026"), None);
}

#[test]
fn a_page_without_audits_is_refused_rather_than_read_as_a_pass() {
    let error = parse_page(URL, "<html><body>nothing here</body></html>")
        .expect_err("no audit section means no import");
    assert!(error.to_string().contains("does not look like"), "{error}");

    let error = parse_page(
        "https://www.skills.sh/vercel-labs/skills/find-skills/security/socket",
        PAGE,
    )
    .expect_err("an audit page is not a skill page");
    assert!(error.to_string().contains("audit page"), "{error}");

    let error = parse_page("https://www.skills.sh/vercel-labs", PAGE)
        .expect_err("a repository page is not a skill page");
    assert!(error.to_string().contains("not a skill page"), "{error}");
}

fn record() -> SkillRecord {
    SkillRecord {
        id: "github:vercel-labs/skills:skills/find-skills/SKILL.md".to_string(),
        name: "find-skills".to_string(),
        description: "Helps users discover and install agent skills".to_string(),
        source: "github".to_string(),
        repo: Some("vercel-labs/skills".to_string()),
        path: Some("skills/find-skills/SKILL.md".to_string()),
        url: Some("https://github.com/vercel-labs/skills".to_string()),
        body_sha: None,
        grade: Some("C".to_string()),
        grade_scope: Some("file-only".to_string()),
        license: None,
        stars: Some(36_200),
        installs: None,
        tokens: None,
        capabilities: Vec::new(),
        simhash: None,
    }
}

fn page() -> SkillsShPage {
    SkillsShPage {
        url: URL.to_string(),
        repo: "vercel-labs/skills".to_string(),
        slug: "find-skills".to_string(),
        first_seen_at: Some(1_769_385_600_000),
        audits: vec![
            AuditVerdict {
                vendor: "Gen Agent Trust Hub".to_string(),
                verdict: "pass".to_string(),
                url: format!("{URL}/security/agent-trust-hub"),
                audited_on: Some("Aug 30, 2026".to_string()),
                audited_at: parse_date_ms("Aug 30, 2026"),
            },
            AuditVerdict {
                vendor: "Socket".to_string(),
                verdict: "pass".to_string(),
                url: format!("{URL}/security/socket"),
                audited_on: Some("Sep 15, 2026".to_string()),
                audited_at: Some(1_789_430_400_000),
            },
            AuditVerdict {
                vendor: "Snyk".to_string(),
                verdict: "warn".to_string(),
                url: format!("{URL}/security/snyk"),
                audited_on: None,
                audited_at: None,
            },
        ],
    }
}

#[tokio::test]
async fn verdicts_are_dated_evidence_and_never_move_the_grade() {
    let tmp = TempDir::new().unwrap();
    let index = SkillIndex::open(tmp.path()).await.unwrap();
    index.write_skill(&record()).await.unwrap();

    let id = record().id;
    let stored = attach(&index, &page(), Some(&id)).await.unwrap();
    assert_eq!(stored, 3);

    let detail = index.get(&id).await.unwrap().expect("the row is there");
    let rows: Vec<(String, String, i64, &str)> = detail
        .evidence
        .iter()
        .map(|row| {
            (
                row.metric.clone(),
                row.label.to_string(),
                row.observed_at,
                row.source.as_str(),
            )
        })
        .collect();

    assert_eq!(
        rows,
        vec![
            (
                "audit:gen=pass".to_string(),
                "audited".to_string(),
                parse_date_ms("Aug 30, 2026").unwrap(),
                "skills.sh"
            ),
            (
                "audit:snyk=warn".to_string(),
                "audited".to_string(),
                rows[1].2,
                "skills.sh"
            ),
            (
                "audit:socket=pass".to_string(),
                "audited".to_string(),
                1_789_430_400_000,
                "skills.sh"
            ),
        ],
        "each verdict is a row with the audit's own date"
    );
    assert!(
        rows[1].2 > 0,
        "an unreadable audit date falls back to the observation time, never to nothing"
    );

    assert_eq!(
        detail.skill.grade.as_deref(),
        Some("C"),
        "a directory's Pass is an input, not a grade (D93)"
    );

    // Re-importing the same page updates the rows instead of piling them up.
    attach(&index, &page(), Some(&id)).await.unwrap();
    let again = index.get(&id).await.unwrap().unwrap();
    assert_eq!(
        again.evidence.len(),
        3,
        "the evidence key is (skill, metric, source)"
    );
}
