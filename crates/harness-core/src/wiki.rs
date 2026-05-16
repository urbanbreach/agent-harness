use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const WIKI_MODE: &str = "workflow.wiki";
pub const WIKI_EVIDENCE_CATEGORY: &str = "evidence.wiki";
pub const WIKI_ARTIFACT_KIND: &str = "workflow_wiki_page";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WikiPage {
    pub slug: String,
    pub title: String,
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub body: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WikiPageSummary {
    pub slug: String,
    pub title: String,
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub path: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WikiLintFinding {
    pub slug: String,
    pub level: String,
    pub message: String,
}

pub fn wiki_page_path(root: &Path, slug: &str) -> Result<PathBuf, String> {
    let safe = sanitize_slug(slug)?;
    Ok(root.join(format!("{safe}.md")))
}

pub fn sanitize_slug(slug: &str) -> Result<String, String> {
    let trimmed = slug.trim().trim_end_matches(".md");
    if trimmed.is_empty() {
        return Err("wiki slug is required".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return Err("wiki slug must be a single file name without traversal".to_string());
    }
    let safe = trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if safe.is_empty() {
        Err("wiki slug must contain an alphanumeric character".to_string())
    } else {
        Ok(safe)
    }
}

pub fn render_wiki_page(title: &str, category: &str, tags: &[String], body: &str) -> String {
    format!(
        "---\ntitle: {}\ncategory: {}\ntags: {}\n---\n\n{}\n",
        title.trim(),
        category.trim(),
        tags.join(", "),
        body.trim_end()
    )
}

pub fn parse_wiki_page(slug: &str, contents: &str) -> WikiPage {
    let (metadata, body) = parse_frontmatter(contents);
    let mut tags = metadata
        .get("tags")
        .map(|tags| {
            tags.split(',')
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tags.sort();
    tags.dedup();
    WikiPage {
        slug: slug.to_string(),
        title: metadata
            .get("title")
            .cloned()
            .unwrap_or_else(|| slug.to_string()),
        category: metadata
            .get("category")
            .cloned()
            .unwrap_or_else(|| "uncategorized".to_string()),
        tags,
        body: body.trim().to_string(),
        digest: wiki_digest(contents),
    }
}

pub fn wiki_summary(slug: &str, path: &Path, contents: &str) -> WikiPageSummary {
    let page = parse_wiki_page(slug, contents);
    WikiPageSummary {
        slug: page.slug,
        title: page.title,
        category: page.category,
        tags: page.tags,
        path: path.display().to_string(),
        digest: page.digest,
    }
}

pub fn wiki_lint(page: &WikiPage) -> Vec<WikiLintFinding> {
    let mut findings = Vec::new();
    if page.title.trim().is_empty() || page.title == page.slug {
        findings.push(WikiLintFinding {
            slug: page.slug.clone(),
            level: "warn".to_string(),
            message: "wiki page should declare a title".to_string(),
        });
    }
    if page.category.trim().is_empty() || page.category == "uncategorized" {
        findings.push(WikiLintFinding {
            slug: page.slug.clone(),
            level: "warn".to_string(),
            message: "wiki page should declare a category".to_string(),
        });
    }
    if page.body.trim().is_empty() {
        findings.push(WikiLintFinding {
            slug: page.slug.clone(),
            level: "error".to_string(),
            message: "wiki page body is empty".to_string(),
        });
    }
    findings
}

pub fn wiki_matches(
    page: &WikiPage,
    term: Option<&str>,
    tag: Option<&str>,
    category: Option<&str>,
) -> bool {
    if let Some(category) = category {
        if !page.category.eq_ignore_ascii_case(category) {
            return false;
        }
    }
    if let Some(tag) = tag {
        if !page
            .tags
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(tag))
        {
            return false;
        }
    }
    if let Some(term) = term {
        let term = term.to_ascii_lowercase();
        let haystack = format!(
            "{}\n{}\n{}\n{}",
            page.title,
            page.category,
            page.tags.join("\n"),
            page.body
        )
        .to_ascii_lowercase();
        if !haystack.contains(&term) {
            return false;
        }
    }
    true
}

pub fn wiki_digest(contents: &str) -> String {
    blake3::hash(contents.as_bytes()).to_hex().to_string()
}

pub fn wiki_evidence_metadata(action: &str, page: &WikiPageSummary) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("artifact_kind".to_string(), WIKI_ARTIFACT_KIND.to_string()),
        ("wiki_action".to_string(), action.to_string()),
        ("page_slug".to_string(), page.slug.clone()),
        ("page_title".to_string(), page.title.clone()),
        ("page_category".to_string(), page.category.clone()),
        ("page_tags".to_string(), page.tags.join(",")),
        ("page_path".to_string(), page.path.clone()),
        ("page_digest".to_string(), page.digest.clone()),
    ])
}

fn parse_frontmatter(contents: &str) -> (BTreeMap<String, String>, String) {
    let mut metadata = BTreeMap::new();
    let mut lines = contents.lines();
    if lines.next() != Some("---") {
        return (metadata, contents.to_string());
    }
    let mut body = Vec::new();
    let mut in_metadata = true;
    for line in lines {
        if in_metadata && line.trim() == "---" {
            in_metadata = false;
            continue;
        }
        if in_metadata {
            if let Some((key, value)) = line.split_once(':') {
                metadata.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        } else {
            body.push(line);
        }
    }
    (metadata, body.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_pages_parse_search_and_lint() {
        let body = render_wiki_page(
            "Workflow Evidence",
            "architecture",
            &["workflow".to_string(), "evidence".to_string()],
            "Replay uses event metadata.",
        );
        let page = parse_wiki_page("workflow-evidence", &body);
        assert_eq!(page.title, "Workflow Evidence");
        assert_eq!(page.category, "architecture");
        assert!(wiki_matches(
            &page,
            Some("event metadata"),
            Some("workflow"),
            Some("architecture")
        ));
        assert!(wiki_lint(&page).is_empty());
    }

    #[test]
    fn wiki_slug_rejects_traversal() {
        assert!(sanitize_slug("../secret").is_err());
        assert_eq!(
            sanitize_slug("Workflow Evidence").unwrap(),
            "workflow-evidence"
        );
    }
}
