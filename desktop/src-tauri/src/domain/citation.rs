//! Citation support (scope doc A6): metadata lookup via the free Crossref API
//! (no key, runs Rust-side so capabilities stay untouched) and template-code
//! formatting to APA / MLA / Chicago — deterministic where users check every
//! character. Pure parts are unit-tested; `fetch_metadata` is the only network.

use serde::{Deserialize, Serialize};

use super::error::{DomainError, DomainResult};

/// One author, split the way Crossref reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub given: String,
}

/// Structured citation metadata — the shared currency between the fetcher and
/// the formatters (and the `fetch_citation_metadata` command's DTO).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CiteMeta {
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub title: String,
    /// Journal / book container title (Crossref `container-title[0]`).
    #[serde(default)]
    pub container: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub volume: String,
    #[serde(default)]
    pub issue: String,
    /// Page range as printed (Crossref `page`, may be "45-56").
    #[serde(default)]
    pub pages: String,
    #[serde(default)]
    pub doi: String,
    #[serde(default)]
    pub url: String,
}

/// The subset of the Crossref `works` message we need. Everything defaults so a
/// sparse record still formats (title-only fallback) instead of failing.
#[derive(Debug, Deserialize)]
struct CrossrefWork {
    #[serde(default, rename = "container-title")]
    container_title: Vec<String>,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default, rename = "DOI")]
    doi: String,
    #[serde(default)]
    volume: String,
    #[serde(default)]
    issue: String,
    #[serde(default, rename = "page")]
    page: String,
    #[serde(default)]
    publisher: String,
    #[serde(default, rename = "URL")]
    url: String,
    #[serde(default, rename = "issued")]
    issued: Option<CrossrefIssued>,
    #[serde(default)]
    author: Vec<Author>,
}

#[derive(Debug, Deserialize)]
struct CrossrefIssued {
    #[serde(default, rename = "date-parts")]
    date_parts: Vec<Vec<i64>>,
}

impl From<CrossrefWork> for CiteMeta {
    fn from(w: CrossrefWork) -> Self {
        CiteMeta {
            authors: w
                .author
                .into_iter()
                .filter(|a| !(a.family.is_empty() && a.given.is_empty()))
                .collect(),
            title: w.title.into_iter().next().unwrap_or_default(),
            container: w.container_title.into_iter().next().unwrap_or_default(),
            publisher: w.publisher,
            year: w
                .issued
                .and_then(|i| i.date_parts.into_iter().next())
                .and_then(|p| p.into_iter().next()),
            volume: w.volume,
            issue: w.issue,
            pages: w.page,
            doi: w.doi,
            url: w.url,
        }
    }
}

/// Parse a Crossref response body (`api.crossref.org/works/...` envelope with a
/// `message` object). Tolerant of missing fields by design.
pub fn parse_crossref_works(raw: &str) -> DomainResult<CiteMeta> {
    let envelope: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| DomainError::Validation(format!("crossref response: {e}")))?;
    let message = envelope.get("message").cloned().unwrap_or(envelope); // tolerate a bare work object too
    let work: CrossrefWork = serde_json::from_value(message)
        .map_err(|e| DomainError::Validation(format!("crossref work: {e}")))?;
    let meta: CiteMeta = work.into();
    if meta.title.is_empty() && meta.authors.is_empty() {
        return Err(DomainError::NotFound("no citation metadata found".into()));
    }
    Ok(meta)
}

/// How a user-provided source should be looked up.
enum Source {
    /// A bare DOI (`10.xxxx/...`) possibly wrapped in a doi.org URL.
    Doi(String),
    /// Free text — a title to search for (best match).
    Title(String),
}

/// Classify the source: anything containing a DOI pattern (optionally inside a
/// doi.org / dx.doi.org / info:doi URL) is a DOI; everything else is a title query.
fn classify_source(source: &str) -> Source {
    let trimmed = source.trim();
    let no_url = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("dx.doi.org/")
        .trim_start_matches("doi.org/")
        .trim_start_matches("info:doi/");
    // The DOI itself: from "10." up to the next whitespace (URLs may carry suffixes).
    if let Some(pos) = no_url.find("10.") {
        let rest = &no_url[pos..];
        let doi: String = rest
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != ')' && *c != '}')
            .collect();
        if doi.len() > 4 {
            return Source::Doi(doi.trim_end_matches('.').to_string());
        }
    }
    Source::Title(trimmed.to_string())
}

/// Look up metadata for a DOI, URL, or raw title. Crossref is free + keyless;
/// failures surface as validation errors naming the source.
pub async fn fetch_metadata(source: &str) -> DomainResult<CiteMeta> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| DomainError::Validation(format!("http client: {e}")))?;
    let (url, query) = match classify_source(source) {
        Source::Doi(doi) => (format!("https://api.crossref.org/works/{doi}"), None),
        Source::Title(title) => (
            "https://api.crossref.org/works".to_string(),
            Some([
                ("query.bibliographic".to_string(), title),
                ("rows".to_string(), "1".to_string()),
            ]),
        ),
    };
    let mut req = client
        .get(&url)
        .header("User-Agent", "PhraseXp/1.7 (local text expander)");
    if let Some(q) = query {
        req = req.query(&q);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| DomainError::Validation(format!("citation lookup: {e}")))?;
    let status = resp.status();
    let raw = resp
        .text()
        .await
        .map_err(|e| DomainError::Validation(format!("citation read: {e}")))?;
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(DomainError::NotFound(format!(
            "no Crossref record for “{source}”"
        )));
    }
    if !status.is_success() {
        return Err(DomainError::Validation(format!(
            "citation lookup failed (HTTP {status})"
        )));
    }
    // Title searches return a list — take the top hit.
    if let Source::Title(_) = classify_source(source) {
        return parse_crossref_search(&raw);
    }
    parse_crossref_works(&raw)
}

/// Parse a `works?query...` list response (top hit of `message.items`).
fn parse_crossref_search(raw: &str) -> DomainResult<CiteMeta> {
    let envelope: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| DomainError::Validation(format!("crossref response: {e}")))?;
    let item = envelope
        .pointer("/message/items/0")
        .cloned()
        .ok_or_else(|| DomainError::NotFound("no Crossref results for that title".into()))?;
    let work: CrossrefWork = serde_json::from_value(item)
        .map_err(|e| DomainError::Validation(format!("crossref work: {e}")))?;
    let meta: CiteMeta = work.into();
    if meta.title.is_empty() && meta.authors.is_empty() {
        return Err(DomainError::NotFound("no citation metadata found".into()));
    }
    Ok(meta)
}

// -- Formatting (pure, unit-tested) ----------------------------------------- //

/// Author-list builders per style — the three house styles invert and elide
/// authors differently. APA inverts every name with initials; MLA/Chicago
/// invert only the first (plus `et al.` thresholds).
///
/// APA 7: `Family, G. I.` — up to 20 authors joined with `, &` before the last.
fn apa_authors(authors: &[Author]) -> String {
    let parts: Vec<String> = authors.iter().take(20).map(apa_name).collect();
    match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        n => format!("{}, & {}", parts[..n - 1].join(", "), parts[n - 1]),
    }
}

fn apa_name(a: &Author) -> String {
    let initials: String = a
        .given
        .split_whitespace()
        .filter_map(|g| g.chars().next())
        .map(|c| format!("{c}."))
        .collect::<Vec<_>>()
        .join(" ");
    if initials.is_empty() {
        a.family.clone()
    } else if a.family.is_empty() {
        initials
    } else {
        format!("{}, {}", a.family, initials)
    }
}

/// MLA 9: `Given Family` — first author inverted, 3+ → first `et al.`
fn mla_authors(authors: &[Author]) -> String {
    match authors.len() {
        0 => String::new(),
        1 => mla_first(authors[0].clone()),
        2 => format!(
            "{}, and {}",
            mla_first(authors[0].clone()),
            mla_rest(&authors[1])
        ),
        _ => format!("{}, et al.", mla_first(authors[0].clone())),
    }
}

fn mla_first(a: Author) -> String {
    if a.family.is_empty() {
        a.given
    } else if a.given.is_empty() {
        a.family
    } else {
        format!("{}, {}", a.family, a.given)
    }
}

fn mla_rest(a: &Author) -> String {
    if a.family.is_empty() {
        a.given.clone()
    } else if a.given.is_empty() {
        a.family.clone()
    } else {
        format!("{} {}", a.given, a.family)
    }
}

/// Chicago (bibliography): like MLA but 10+ → first `et al.`; 2-10 all listed.
fn chicago_authors(authors: &[Author]) -> String {
    match authors.len() {
        0 => String::new(),
        1 => mla_first(authors[0].clone()),
        n if n >= 10 => format!("{} et al.", mla_first(authors[0].clone())),
        _ => {
            let mut parts: Vec<String> = vec![mla_first(authors[0].clone())];
            for a in &authors[1..] {
                parts.push(mla_rest(a));
            }
            let last = parts.pop().unwrap_or_default();
            // Chicago uses the serial comma: "A, B, and C".
            format!("{}, and {}", parts.join(", "), last)
        }
    }
}

fn apa_year(meta: &CiteMeta) -> String {
    meta.year.map(|y| format!("({y}). ")).unwrap_or_default()
}

fn link(meta: &CiteMeta) -> String {
    if !meta.doi.is_empty() {
        format!("https://doi.org/{}", meta.doi)
    } else {
        meta.url.clone()
    }
}

/// Format to one of the three house styles. Unknown style → validation error.
pub fn format_citation(meta: &CiteMeta, style: &str) -> DomainResult<String> {
    match style {
        "apa" => Ok(format_apa(meta)),
        "mla" => Ok(format_mla(meta)),
        "chicago" => Ok(format_chicago(meta)),
        other => Err(DomainError::Validation(format!(
            "unknown citation style “{other}” (apa, mla, chicago)"
        ))),
    }
}

/// APA 7: `Author, A. (Year). Title. Container, Vol(Issue), Pages. link`
/// Books (no container): `Author. (Year). Title. Publisher.`
fn format_apa(meta: &CiteMeta) -> String {
    let mut out = String::new();
    let a = apa_authors(&meta.authors);
    if !a.is_empty() {
        out.push_str(&a);
        out.push(' ');
    }
    out.push_str(&apa_year(meta));
    out.push_str(&meta.title);
    if !meta.title.is_empty() && !meta.title.ends_with('.') {
        out.push('.');
    }
    if !meta.container.is_empty() {
        // Journal segment: `Container, Vol(Issue), Pages.` — one trailing period,
        // so the volume/pages punctuation must not double up.
        let mut seg = format!(" {}", meta.container);
        if !meta.volume.is_empty() {
            seg.push_str(&format!(", {}", meta.volume));
            if !meta.issue.is_empty() {
                seg.push_str(&format!("({})", meta.issue));
            }
        }
        if !meta.pages.is_empty() {
            let sep = if meta.volume.is_empty() { ". " } else { ", " };
            seg.push_str(&format!("{sep}{}", meta.pages.replace('-', "–")));
        }
        seg.push('.');
        out.push_str(&seg);
    } else if !meta.publisher.is_empty() {
        out.push_str(&format!(" {}.", meta.publisher));
    }
    let l = link(meta);
    if !l.is_empty() {
        if !out.ends_with(' ') {
            out.push(' ');
        }
        out.push_str(&l);
    }
    out
}

/// MLA 9: `Author. "Title." Container, vol. V, no. I, Year, pp. P. link.`
fn format_mla(meta: &CiteMeta) -> String {
    let mut out = String::new();
    let a = mla_authors(&meta.authors);
    if !a.is_empty() {
        out.push_str(&a);
        if !out.ends_with('.') {
            out.push('.');
        }
        out.push(' ');
    }
    if !meta.title.is_empty() {
        out.push_str(&format!("“{}.” ", meta.title));
    }
    if !meta.container.is_empty() {
        out.push_str(&meta.container);
        if !meta.volume.is_empty() {
            out.push_str(&format!(", vol. {}", meta.volume));
        }
        if !meta.issue.is_empty() {
            out.push_str(&format!(", no. {}", meta.issue));
        }
        if let Some(y) = meta.year {
            out.push_str(&format!(", {y}"));
        }
        if !meta.pages.is_empty() {
            out.push_str(&format!(", pp. {}", meta.pages.replace('-', "–")));
        }
        out.push_str(". ");
    } else if !meta.publisher.is_empty() {
        if let Some(y) = meta.year {
            out.push_str(&format!("{y}. "));
        }
        out.push_str(&format!("{}. ", meta.publisher));
    }
    let l = link(meta);
    if !l.is_empty() {
        out.push_str(&l);
        out.push('.');
    }
    out.trim_end().to_string()
}

/// Chicago (bibliography): `Author. "Title." Container V, no. I (Year): P. link.`
fn format_chicago(meta: &CiteMeta) -> String {
    let mut out = String::new();
    let a = chicago_authors(&meta.authors);
    if !a.is_empty() {
        out.push_str(&a);
        if !out.ends_with('.') {
            out.push('.');
        }
        out.push(' ');
    }
    if !meta.title.is_empty() {
        out.push_str(&format!("“{}.” ", meta.title));
    }
    if !meta.container.is_empty() {
        out.push_str(&meta.container);
        if !meta.volume.is_empty() {
            out.push_str(&format!(" {}", meta.volume));
        }
        if !meta.issue.is_empty() {
            out.push_str(&format!(", no. {}", meta.issue));
        }
        if let Some(y) = meta.year {
            out.push_str(&format!(" ({y})"));
        }
        if !meta.pages.is_empty() {
            let sep = if meta.year.is_some() { ": " } else { " " };
            out.push_str(&format!("{sep}{}", meta.pages.replace('-', "–")));
        }
        out.push_str(". ");
    } else if !meta.publisher.is_empty() {
        if let Some(y) = meta.year {
            out.push_str(&format!("{y}. "));
        }
        out.push_str(&format!("{}. ", meta.publisher));
    }
    let l = link(meta);
    if !l.is_empty() {
        out.push_str(&l);
        out.push('.');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> CiteMeta {
        CiteMeta {
            authors: vec![
                Author {
                    family: "Vaswani".into(),
                    given: "Ashish".into(),
                },
                Author {
                    family: "Shazeer".into(),
                    given: "Noam".into(),
                },
                Author {
                    family: "Parmar".into(),
                    given: "Niki".into(),
                },
            ],
            title: "Attention is all you need".into(),
            container: "Advances in Neural Information Processing Systems".into(),
            publisher: String::new(),
            year: Some(2017),
            volume: "30".into(),
            issue: String::new(),
            pages: "5998-6008".into(),
            doi: "10.5555/3295222.3295349".into(),
            url: String::new(),
        }
    }

    #[test]
    fn apa_format() {
        assert_eq!(
            format_citation(&meta(), "apa").unwrap(),
            "Vaswani, A., Shazeer, N., & Parmar, N. (2017). Attention is all you need. \
             Advances in Neural Information Processing Systems, 30, 5998–6008. \
             https://doi.org/10.5555/3295222.3295349"
        );
    }

    #[test]
    fn mla_format() {
        assert_eq!(
            format_citation(&meta(), "mla").unwrap(),
            "Vaswani, Ashish, et al. “Attention is all you need.” \
             Advances in Neural Information Processing Systems, vol. 30, 2017, \
             pp. 5998–6008. https://doi.org/10.5555/3295222.3295349."
        );
    }

    #[test]
    fn chicago_format() {
        assert_eq!(
            format_citation(&meta(), "chicago").unwrap(),
            "Vaswani, Ashish, Noam Shazeer, and Niki Parmar. “Attention is all you need.” \
             Advances in Neural Information Processing Systems 30 (2017): 5998–6008. \
             https://doi.org/10.5555/3295222.3295349."
        );
    }

    #[test]
    fn two_authors_join_differently_per_style() {
        let two = CiteMeta {
            authors: vec![
                Author {
                    family: "Rowling".into(),
                    given: "Joanne".into(),
                },
                Author {
                    family: "Tolkien".into(),
                    given: "John".into(),
                },
            ],
            title: "A book".into(),
            publisher: "Print".into(),
            year: Some(2000),
            ..Default::default()
        };
        let apa = format_citation(&two, "apa").unwrap();
        assert!(apa.starts_with("Rowling, J., & Tolkien, J. (2000). A book. Print."));
        let mla = format_citation(&two, "mla").unwrap();
        assert!(mla.starts_with("Rowling, Joanne, and John Tolkien. “A book.” 2000. Print."));
        let chi = format_citation(&two, "chicago").unwrap();
        assert!(chi.starts_with("Rowling, Joanne, and John Tolkien. “A book.” 2000. Print."));
    }

    #[test]
    fn no_authors_starts_with_title() {
        let none = CiteMeta {
            title: "Anonymous work".into(),
            year: Some(1999),
            ..Default::default()
        };
        let apa = format_citation(&none, "apa").unwrap();
        assert!(apa.starts_with("(1999). Anonymous work."));
    }

    #[test]
    fn unknown_style_errors() {
        assert!(format_citation(&meta(), "ieee").is_err());
    }

    const WORK_JSON: &str = r#"{
        "message": {
            "title": ["Attention is all you need"],
            "container-title": ["NeurIPS"],
            "DOI": "10.5555/3295222",
            "volume": "30",
            "page": "5998-6008",
            "issued": { "date-parts": [[2017, 6, 12]] },
            "author": [
                { "family": "Vaswani", "given": "Ashish" },
                { "family": "Shazeer", "given": "Noam" }
            ],
            "publisher": "Curran"
        }
    }"#;

    #[test]
    fn parses_crossref_envelope() {
        let m = parse_crossref_works(WORK_JSON).unwrap();
        assert_eq!(m.title, "Attention is all you need");
        assert_eq!(m.container, "NeurIPS");
        assert_eq!(m.year, Some(2017));
        assert_eq!(m.authors.len(), 2);
        assert_eq!(m.doi, "10.5555/3295222");
    }

    #[test]
    fn parses_sparse_work() {
        let m = parse_crossref_works(r#"{"message": {"title": ["Bare"]}}"#).unwrap();
        assert_eq!(m.title, "Bare");
        assert!(m.authors.is_empty());
        assert_eq!(m.year, None);
    }

    #[test]
    fn empty_work_errors() {
        assert!(parse_crossref_works(r#"{"message": {}}"#).is_err());
        assert!(parse_crossref_works("not json").is_err());
    }

    #[test]
    fn classifies_sources() {
        match classify_source("https://doi.org/10.5555/3295222") {
            Source::Doi(d) => assert_eq!(d, "10.5555/3295222"),
            _ => panic!("expected Doi"),
        }
        match classify_source("10.5555/3295222.3295349") {
            Source::Doi(d) => assert_eq!(d, "10.5555/3295222.3295349"),
            _ => panic!("expected Doi"),
        }
        match classify_source("Attention is all you need") {
            Source::Title(t) => assert_eq!(t, "Attention is all you need"),
            _ => panic!("expected Title"),
        }
    }
}
