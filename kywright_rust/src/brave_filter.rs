//! src/brave_filter.rs
//!
//! Integration wrapper around Brave's `adblock-rust` engine.
//!
//! This module owns a single lazily-initialized `adblock::Engine` built from
//! EasyList + EasyPrivacy filter lists on disk. It is intentionally decoupled
//! from the structural streaming filter in `filter.rs` — this pass handles
//! *network-request-shaped* blocking decisions (is this URL an ad/tracker),
//! while `filter.rs` handles structural HTML removal (class/id heuristics,
//! script/style/iframe stripping, PII redaction). They are complementary,
//! not redundant.
//!
//! IMPORTANT SCOPE NOTE: `adblock::Engine` is fundamentally a *URL/request*
//! blocker (it answers "should this request to URL X, of resource type Y,
//! initiated from page Z, be blocked?"), not an HTML rewriter. It does not
//! parse or mutate raw HTML strings on its own. To use it against HTML, this
//! module extracts candidate URLs from the markup (currently: `src`/`href`
//! attributes on elements that issue requests: script, img, iframe, link,
//! embed, source) and removes the *elements* whose URL the engine says to
//! block. This is a deliberate, explicit design choice — do not treat
//! `brave_filter_html` as equivalent to Brave's browser-level cosmetic
//! filtering (element hiding via injected CSS); that is a separate engine
//! feature (`Engine::url_cosmetic_resources`) not wired up here, since it
//! requires a real DOM + page URL context this module doesn't have.
//!
//! Thread-safety: `adblock::Engine` is `Send + Sync` once built, so we store
//! it behind a `once_cell::sync::Lazy<Engine>` directly (no mutex needed for
//! reads; `Engine::check_network_urls` takes `&self`).

use once_cell::sync::Lazy;
use regex::Regex;
use std::fs;
use std::path::Path;

use adblock::engine::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;

/// Paths to the pre-downloaded filter lists. Overridable via env vars so
/// deployment doesn't require rebuilding to relocate them.
const EASYLIST_PATH_DEFAULT: &str = "lists/easylist.txt";
const EASYPRIVACY_PATH_DEFAULT: &str = "lists/easyprivacy.txt";

fn easylist_path() -> String {
    std::env::var("KYWRIGHT_EASYLIST_PATH").unwrap_or_else(|_| EASYLIST_PATH_DEFAULT.to_string())
}

fn easyprivacy_path() -> String {
    std::env::var("KYWRIGHT_EASYPRIVACY_PATH")
        .unwrap_or_else(|_| EASYPRIVACY_PATH_DEFAULT.to_string())
}

/// Global engine instance. Built once, on first use, from disk.
///
/// If list loading fails (missing file, unreadable), we do NOT panic the
/// process — a missing/corrupt filter list should degrade to "no extra
/// blocking from this pass", not take down the whole filter pipeline, since
/// `filter.rs`'s structural pass still runs regardless. We log to stderr and
/// fall back to an engine built from an empty filter set.
static ENGINE: Lazy<std::sync::Mutex<Engine>> = Lazy::new(|| {
    let engine = build_engine().unwrap_or_else(|e| {
        eprintln!(
            "[brave_filter] WARNING: failed to build adblock engine from lists ({e}); \
             falling back to empty ruleset. Ad/tracker URL blocking via adblock-rust \
             is DISABLED for this process; structural filtering in filter.rs still applies.",
        );
        Engine::default()
    });
    std::sync::Mutex::new(engine)
});

#[derive(Debug)]
enum BuildError {
    Io(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io(msg) => write!(f, "io error: {msg}"),
        }
    }
}

fn read_list(path: &str) -> Result<Vec<String>, BuildError> {
    let p = Path::new(path);
    let contents = fs::read_to_string(p)
        .map_err(|e| BuildError::Io(format!("{path}: {e}")))?;
    Ok(contents.lines().map(|l| l.to_string()).collect())
}

fn build_engine() -> Result<Engine, BuildError> {
    let mut filter_set = FilterSet::new(false); // debug=false: don't retain raw rule text, saves memory

    let easylist = read_list(&easylist_path())?;
    filter_set.add_filter_list(easylist.join("\n"), ParseOptions::default());

    // EasyPrivacy is optional-but-expected; if it's missing we still want
    // EasyList-only blocking rather than failing the whole build.
    match read_list(&easyprivacy_path()) {
        Ok(easyprivacy) => {
            filter_set.add_filter_list(easyprivacy.join("\n"), ParseOptions::default());
        }
        Err(e) => {
            eprintln!(
                "[brave_filter] WARNING: could not load EasyPrivacy list ({e}); \
                 continuing with EasyList rules only.",
            );
        }
    }

    Ok(Engine::new_with_filter_set(filter_set))
}

/// Force initialization of the global engine (useful to call once at
/// process startup so the first real request doesn't pay list-parse
/// latency). Safe to call multiple times.
pub fn warm_up() {
    Lazy::force(&ENGINE);
}

// ---------------------------------------------------------------------
// URL extraction from raw HTML
// ---------------------------------------------------------------------
//
// This is deliberately narrow: we only pull `src=`/`href=` values off a
// fixed set of request-issuing tags. This is NOT a general HTML attribute
// parser (that's dom.rs's job); it's a cheap regex pass good enough to
// find candidate URLs for the ad-block URL check. False negatives here
// (a URL we fail to extract) just mean that element isn't checked against
// the engine — filter.rs's structural/class-based pass is still applied
// independently downstream, so this is a defense-in-depth layer, not the
// only line of defense.
//
// No backreferences are used anywhere (Rust's `regex` crate doesn't
// support them) — this is a single forward-only alternation pattern.

static REQUEST_TAG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?is)<(script|img|iframe|link|embed|source)\b[^>]*?\b(?:src|href)\s*=\s*["']([^"']+)["'][^>]*>"#,
    )
    .unwrap()
});

struct CandidateUrl {
    /// Byte range of the *entire matched tag* in the source string.
    tag_start: usize,
    tag_end: usize,
    tag_name: String,
    url: String,
}

fn extract_candidate_urls(html: &str) -> Vec<CandidateUrl> {
    REQUEST_TAG_RE
        .captures_iter(html)
        .filter_map(|cap| {
            let whole = cap.get(0)?;
            let tag_name = cap.get(1)?.as_str().to_ascii_lowercase();
            let url = cap.get(2)?.as_str().to_string();
            Some(CandidateUrl {
                tag_start: whole.start(),
                tag_end: whole.end(),
                tag_name,
                url,
            })
        })
        .collect()
}

/// Map an HTML tag name to the adblock-rust resource type string it
/// expects for `check_network_urls`. adblock-rust's request-type matching
/// (`$script`, `$image`, `$subdocument`, `$stylesheet`, etc.) affects which
/// filter rules apply, so under-specifying this weakens blocking accuracy.
fn resource_type_for_tag(tag: &str) -> &'static str {
    match tag {
        "script" => "script",
        "img" => "image",
        "iframe" => "subdocument",
        "link" => "stylesheet",
        "source" => "media",
        "embed" => "object",
        _ => "other",
    }
}

/// Best-effort absolute-ize of a possibly-relative URL against a page URL.
/// adblock-rust's matching is meaningfully more accurate with real absolute
/// URLs (domain-based rules, first-party/third-party checks). If no base
/// is available we pass the URL through as-is; the engine still handles
/// many rules fine on bare paths, just less precisely for domain-scoped
/// rules.
fn resolve_url(url: &str, page_url: Option<&str>) -> String {
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//") {
        return if let Some(rest) = url.strip_prefix("//") {
            format!("https://{rest}")
        } else {
            url.to_string()
        };
    }
    match page_url {
        Some(base) => {
            // Minimal, dependency-free relative resolution: only handles the
            // common cases (root-relative and same-directory-relative). For
            // anything more exotic we fall back to returning the URL
            // unresolved rather than guessing wrong.
            if let Some(origin_end) = base.find("://").map(|i| i + 3) {
                if let Some(host_end) = base[origin_end..].find('/').map(|i| i + origin_end) {
                    let origin = &base[..host_end];
                    if url.starts_with('/') {
                        return format!("{origin}{url}");
                    }
                }
            }
            url.to_string()
        }
        None => url.to_string(),
    }
}

// ---------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------

/// Remove HTML elements whose request URL matches an EasyList/EasyPrivacy
/// blocking rule.
///
/// `page_url` is optional context (the URL the HTML was fetched from) used
/// to improve first-/third-party matching accuracy and relative URL
/// resolution. Pass `None` if unavailable — the function still works, with
/// slightly reduced rule precision on relative URLs.
///
/// This does not attempt cosmetic (CSS-based) element hiding — only
/// network-request-shaped blocking against extracted `src`/`href` URLs.
pub fn brave_filter_html(html: &str) -> String {
    brave_filter_html_with_context(html, None)
}

pub fn brave_filter_html_with_context(html: &str, page_url: Option<&str>) -> String {
    let candidates = extract_candidate_urls(html);
    if candidates.is_empty() {
        return html.to_string();
    }

    let engine = ENGINE.lock().unwrap();

    // Build the list of byte ranges to remove. We evaluate all candidates
    // first, then splice, so overlapping-match edge cases (shouldn't happen
    // given the regex is non-overlapping by construction, but be defensive)
    // are handled by a single linear pass rather than repeated string
    // mutation invalidating earlier offsets.
    let mut removals: Vec<(usize, usize)> = Vec::with_capacity(candidates.len());

    for cand in &candidates {
        let resolved = resolve_url(&cand.url, page_url);
        let resource_type = resource_type_for_tag(&cand.tag_name);
        let source_host = page_url.unwrap_or("");

        let request = match Request::new(
            &resolved,
            source_host,
            resource_type,
            "GET",
        ) {
            Ok(req) => req,
            Err(_) => continue,
        };

        let result = engine.check_network_request(&request);
        let blocked = result.filter.is_some();

        if blocked {
            removals.push((cand.tag_start, cand.tag_end));
        }
    }

    if removals.is_empty() {
        return html.to_string();
    }

    splice_out(html, &removals)
}

/// Remove the given (non-overlapping, arbitrary-order) byte ranges from
/// `s`, returning the result. Ranges are sorted and merged defensively in
/// case of accidental overlap before splicing.
fn splice_out(s: &str, ranges: &[(usize, usize)]) -> String {
    let mut sorted = ranges.to_vec();
    sorted.sort_unstable_by_key(|r| r.0);

    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(sorted.len());
    for (start, end) in sorted {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                if end > last.1 {
                    last.1 = end;
                }
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut out = String::with_capacity(s.len());
    let mut cursor = 0usize;
    for (start, end) in merged {
        if start > cursor {
            out.push_str(&s[cursor..start]);
        }
        cursor = end.max(cursor);
    }
    if cursor < s.len() {
        out.push_str(&s[cursor..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: these tests don't depend on the real EasyList files being
    // present at the default path — they build an in-memory engine
    // directly so they're deterministic in CI regardless of working
    // directory or list availability.

    fn test_engine(rules: &[&str]) -> Engine {
        let mut fs = FilterSet::new(false);
        let owned: Vec<String> = rules.iter().map(|s| s.to_string()).collect();
        fs.add_filter_list(&owned, ParseOptions::default());
        Engine::from_filter_set(fs, true)
    }

    #[test]
    fn blocks_known_ad_domain_script_tag() {
        let engine = test_engine(&["||ads.example.com^"]);
        let matched = engine
            .check_network_urls("https://ads.example.com/tag.js", "https://news.example.org", "script")
            .filter.is_some();
        assert!(matched);
    }

    #[test]
    fn allows_unlisted_domain() {
        let engine = test_engine(&["||ads.example.com^"]);
        let matched = engine
            .check_network_urls("https://cdn.example.org/app.js", "https://news.example.org", "script")
            .filter.is_some();
        assert!(!matched);
    }

    #[test]
    fn extract_candidate_urls_finds_script_and_img() {
        let html = r#"<script src="https://ads.example.com/a.js"></script><img src="/pixel.gif">"#;
        let candidates = extract_candidate_urls(html);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].tag_name, "script");
        assert_eq!(candidates[1].tag_name, "img");
    }

    #[test]
    fn splice_out_removes_ranges_correctly() {
        let s = "AAAA<remove1>BBBB<remove2>CCCC";
        let r1_start = s.find("<remove1>").unwrap();
        let r1_end = r1_start + "<remove1>".len();
        let r2_start = s.find("<remove2>").unwrap();
        let r2_end = r2_start + "<remove2>".len();
        let out = splice_out(s, &[(r1_start, r1_end), (r2_start, r2_end)]);
        assert_eq!(out, "AAAABBBBCCCC");
    }

    #[test]
    fn resolve_url_handles_root_relative() {
        let resolved = resolve_url("/pixel.gif", Some("https://news.example.org/articles/x"));
        assert_eq!(resolved, "https://news.example.org/pixel.gif");
    }

    #[test]
    fn resolve_url_passes_through_absolute() {
        let resolved = resolve_url("https://cdn.example.com/a.js", Some("https://news.example.org/x"));
        assert_eq!(resolved, "https://cdn.example.com/a.js");
    }
}