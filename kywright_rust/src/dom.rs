//! dom.rs — Production-grade DOM parsing module for the Kyvelix engine.
//!
//! Provides a C ABI (via cgo-compatible `extern "C"` functions) around the
//! `scraper` (built on `html5ever`) crate for fast, safe HTML parsing.
//!
//! # Design notes
//! - All fallible/boundary-crossing work happens behind `catch_unwind` so a
//!   panic inside parsing logic can never unwind across the FFI boundary
//!   (which is undefined behavior in Rust).
//! - Every function that returns a `*mut c_char` allocates via
//!   `CString::into_raw()`. The caller (Go) MUST free every non-null pointer
//!   returned from this module by calling `dom_free_string`.
//! - Null / invalid UTF-8 input pointers are handled defensively and produce
//!   a well-formed JSON error object rather than crashing.
//! - No `unwrap()`/`expect()` on data derived from caller input.
//!
//! # Cargo.toml (reference)
//! ```toml
//! [dependencies]
//! scraper = "0.19"
//! html5ever = "0.27"
//! serde = { version = "1", features = ["derive"] }
//! serde_json = "1"
//! ```

use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{self, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// Output data structures
// ---------------------------------------------------------------------------

#[derive(Serialize, Default)]
struct PageSummary {
    title: Option<String>,
    meta_description: Option<String>,
    headings: Headings,
    links: Vec<String>,
    text: String,
}

#[derive(Serialize, Default)]
struct Headings {
    h1: Vec<String>,
    h2: Vec<String>,
    h3: Vec<String>,
}

#[derive(Serialize, Default)]
struct ElementInfo {
    tag: String,
    text: String,
    html: String,
    href: Option<String>,
    src: Option<String>,
    id: Option<String>,
    class: Option<String>,
}

#[derive(Serialize)]
struct ErrorPayload {
    error: String,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert a raw C string pointer to an owned Rust `String`.
///
/// Returns `Err` (with a human-readable reason) if the pointer is null or
/// the bytes are not valid UTF-8. Never panics.
unsafe fn cstr_to_string(ptr: *const c_char) -> Result<String, &'static str> {
    if ptr.is_null() {
        return Err("null pointer passed for string argument");
    }
    let c_str = CStr::from_ptr(ptr);
    match c_str.to_str() {
        Ok(s) => Ok(s.to_owned()),
        Err(_) => Err("input is not valid UTF-8"),
    }
}

/// Serialize a value to a JSON string, falling back to a minimal error JSON
/// object if serialization itself somehow fails (should be effectively
/// unreachable for our own well-formed structs, but we never trust `unwrap`
/// across FFI).
fn to_json_string<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|e| {
        // Fallback hand-built JSON in case serde_json ever fails on our own
        // known-serializable types (e.g. exotic error), so we still return
        // *something* well-formed rather than an empty/invalid string.
        format!(
            "{{\"error\":\"serialization failure: {}\"}}",
            e.to_string().replace('\\', "\\\\").replace('"', "\\\"")
        )
    })
}

fn error_json(msg: &str) -> String {
    to_json_string(&ErrorPayload {
        error: msg.to_string(),
    })
}

/// Allocate a Rust `String` as a C string the caller owns. This is the only
/// sanctioned way strings leave this module — always via `CString::into_raw`.
fn string_to_c_char(s: String) -> *mut c_char {
    // Strip interior NUL bytes defensively: CString::new fails on embedded
    // NULs, which would otherwise force us to unwrap or leak. We never trust
    // that HTML content (or serialized JSON of it) is NUL-free.
    let sanitized = if s.as_bytes().contains(&0) {
        s.replace('\0', "")
    } else {
        s
    };
    match CString::new(sanitized) {
        Ok(cstring) => cstring.into_raw(),
        Err(_) => {
            // Extremely defensive fallback; should not be reachable given the
            // sanitization above.
            CString::new("{\"error\":\"internal NUL sanitization failure\"}")
                .expect("static string is NUL-free")
                .into_raw()
        }
    }
}

/// Run `f` behind `catch_unwind`, returning an error-JSON `CString` pointer
/// if `f` panics. This is the single choke point that guarantees no Rust
/// panic ever unwinds across the FFI boundary into Go.
fn guarded_call<F>(f: F) -> *mut c_char
where
    F: FnOnce() -> String,
{
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(json) => string_to_c_char(json),
        Err(_) => string_to_c_char(error_json("internal panic during DOM parsing")),
    }
}

/// Parse a selector string safely, returning `None` on invalid CSS syntax
/// rather than panicking (scraper's `Selector::parse` returns a `Result`,
/// but we centralize the error message here).
fn parse_selector(selector: &str) -> Result<Selector, String> {
    Selector::parse(selector).map_err(|e| format!("invalid CSS selector: {:?}", e))
}

/// Extract visible text from an element, collapsing whitespace runs into
/// single spaces and trimming the result. Skips `<script>` and `<style>`
/// content explicitly (scraper's `.text()` only yields text nodes, but
/// script/style children are still text nodes in the DOM tree, so we must
/// filter by ancestor tag).
fn clean_visible_text(root: ElementRef) -> String {
    let skip_tags = ["script", "style", "noscript", "template"];

    let mut buf = String::new();
    collect_text_skipping(root, &skip_tags, &mut buf);

    // Collapse all whitespace (including newlines/tabs) into single spaces.
    let collapsed: Vec<&str> = buf.split_whitespace().collect();
    collapsed.join(" ")
}

fn collect_text_skipping(node: ElementRef, skip_tags: &[&str], buf: &mut String) {
    let tag_name = node.value().name();
    if skip_tags.contains(&tag_name) {
        return;
    }
    for child in node.children() {
        if let Some(text) = child.value().as_text() {
            buf.push_str(text);
            buf.push(' ');
        } else if let Some(el) = ElementRef::wrap(child) {
            collect_text_skipping(el, skip_tags, buf);
        }
    }
}

/// Extract an element's own text content only (used for headings, links,
/// query results) — collapsed and trimmed.
fn element_text(el: &ElementRef) -> String {
    let raw: String = el.text().collect::<Vec<_>>().join(" ");
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn element_html(el: &ElementRef) -> String {
    el.html()
}

fn attr<'a>(el: &'a ElementRef, name: &str) -> Option<String> {
    el.value().attr(name).map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Core parsing routines (pure Rust; no FFI concerns below this line)
// ---------------------------------------------------------------------------

fn build_page_summary(html_input: &str) -> PageSummary {
    let document = Html::parse_document(html_input);

    let title_sel = parse_selector("title").ok();
    let meta_desc_sel = parse_selector(r#"meta[name="description" i]"#).ok();
    let h1_sel = parse_selector("h1").ok();
    let h2_sel = parse_selector("h2").ok();
    let h3_sel = parse_selector("h3").ok();
    let link_sel = parse_selector("a[href]").ok();
    let body_sel = parse_selector("body").ok();

    let title = title_sel.and_then(|sel| {
        document
            .select(&sel)
            .next()
            .map(|el| element_text(&el))
            .filter(|s| !s.is_empty())
    });

    let meta_description = meta_desc_sel.and_then(|sel| {
        document
            .select(&sel)
            .next()
            .and_then(|el| attr(&el, "content"))
            .filter(|s| !s.is_empty())
    });

    let mut headings = Headings::default();
    if let Some(sel) = h1_sel {
        headings.h1 = document
            .select(&sel)
            .map(|el| element_text(&el))
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(sel) = h2_sel {
        headings.h2 = document
            .select(&sel)
            .map(|el| element_text(&el))
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(sel) = h3_sel {
        headings.h3 = document
            .select(&sel)
            .map(|el| element_text(&el))
            .filter(|s| !s.is_empty())
            .collect();
    }

    let links: Vec<String> = link_sel
        .map(|sel| {
            document
                .select(&sel)
                .filter_map(|el| attr(&el, "href"))
                .filter(|href| !href.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let text = body_sel
        .and_then(|sel| document.select(&sel).next())
        .map(clean_visible_text)
        .unwrap_or_else(|| {
            // Fallback: no <body> found (fragment input) — use document root.
            document
                .root_element()
                .children()
                .filter_map(ElementRef::wrap)
                .map(clean_visible_text)
                .collect::<Vec<_>>()
                .join(" ")
        });

    PageSummary {
        title,
        meta_description,
        headings,
        links,
        text,
    }
}

fn build_query_results(html_input: &str, selector_str: &str) -> Result<Vec<ElementInfo>, String> {
    let document = Html::parse_document(html_input);
    let selector = parse_selector(selector_str)?;

    let results = document
        .select(&selector)
        .map(|el| ElementInfo {
            tag: el.value().name().to_string(),
            text: element_text(&el),
            html: element_html(&el),
            href: attr(&el, "href"),
            src: attr(&el, "src"),
            id: attr(&el, "id"),
            class: attr(&el, "class"),
        })
        .collect();

    Ok(results)
}

fn build_element_by_id(html_input: &str, id: &str) -> Option<ElementInfo> {
    // CSS.escape-lite: ids can contain characters invalid in unescaped CSS
    // selectors (spaces, quotes, etc.). Rather than hand-rolling full CSS
    // escaping, we match by direct attribute walk to be robust regardless
    // of the id's contents.
    let document = Html::parse_document(html_input);
    let any_sel = parse_selector("[id]").ok()?;

    document
        .select(&any_sel)
        .find(|el| attr(el, "id").as_deref() == Some(id))
        .map(|el| ElementInfo {
            tag: el.value().name().to_string(),
            text: element_text(&el),
            html: element_html(&el),
            href: attr(&el, "href"),
            src: attr(&el, "src"),
            id: attr(&el, "id"),
            class: attr(&el, "class"),
        })
}

fn build_elements_by_class(html_input: &str, class: &str) -> Vec<ElementInfo> {
    let document = Html::parse_document(html_input);
    let any_sel = match parse_selector("[class]") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    document
        .select(&any_sel)
        .filter(|el| {
            el.value()
                .attr("class")
                .map(|c| c.split_whitespace().any(|token| token == class))
                .unwrap_or(false)
        })
        .map(|el| ElementInfo {
            tag: el.value().name().to_string(),
            text: element_text(&el),
            html: element_html(&el),
            href: attr(&el, "href"),
            src: attr(&el, "src"),
            id: attr(&el, "id"),
            class: attr(&el, "class"),
        })
        .collect()
}

/// Extract every URL-bearing attribute relevant to crawling: `<a href>`,
/// `<link href>`, `<img src>`, `<script src>`, `<iframe src>`, and
/// `<source src>`. Deduplicated, order-preserving, empty values dropped.
fn build_extracted_urls(html_input: &str) -> Vec<String> {
    let document = Html::parse_document(html_input);

    let sources: &[(&str, &str)] = &[
        ("a[href]", "href"),
        ("link[href]", "href"),
        ("img[src]", "src"),
        ("script[src]", "src"),
        ("iframe[src]", "src"),
        ("source[src]", "src"),
        ("form[action]", "action"),
    ];

    let mut seen = std::collections::HashSet::new();
    let mut urls = Vec::new();

    for (css, attr_name) in sources {
        if let Ok(sel) = parse_selector(css) {
            for el in document.select(&sel) {
                if let Some(val) = attr(&el, attr_name) {
                    let trimmed = val.trim();
                    if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                        urls.push(trimmed.to_string());
                    }
                }
            }
        }
    }

    urls
}

// ---------------------------------------------------------------------------
// C ABI surface
// ---------------------------------------------------------------------------

/// Parse `input` (HTML, NUL-terminated UTF-8) and return a JSON string
/// summarizing the page: title, meta description, headings, links, and
/// cleaned visible text.
///
/// # Safety
/// `input` must be either null or a valid pointer to a NUL-terminated,
/// UTF-8 C string that remains valid for the duration of this call.
/// The returned pointer (if non-null) must eventually be passed to
/// `dom_free_string` exactly once.
#[no_mangle]
pub unsafe extern "C" fn dom_parse(input: *const c_char) -> *mut c_char {
    let html_input = match cstr_to_string(input) {
        Ok(s) => s,
        Err(e) => return string_to_c_char(error_json(e)),
    };

    guarded_call(move || {
        let summary = build_page_summary(&html_input);
        to_json_string(&summary)
    })
}

/// Run a CSS `selector` against `input` HTML and return a JSON array of
/// matching elements (`tag`, `text`, `html`, `href`, `src`, `id`, `class`).
///
/// # Safety
/// `input` and `selector` must each be either null or valid pointers to
/// NUL-terminated, UTF-8 C strings valid for the duration of this call.
/// The returned pointer (if non-null) must eventually be passed to
/// `dom_free_string` exactly once.
#[no_mangle]
pub unsafe extern "C" fn dom_query_selector(
    input: *const c_char,
    selector: *const c_char,
) -> *mut c_char {
    let html_input = match cstr_to_string(input) {
        Ok(s) => s,
        Err(e) => return string_to_c_char(error_json(e)),
    };
    let selector_str = match cstr_to_string(selector) {
        Ok(s) => s,
        Err(e) => return string_to_c_char(error_json(e)),
    };

    guarded_call(move || match build_query_results(&html_input, &selector_str) {
        Ok(results) => to_json_string(&results),
        Err(e) => error_json(&e),
    })
}

/// Find the element with the given `id` in `input` HTML and return it as a
/// JSON object, or a JSON `null` if not found.
///
/// # Safety
/// `input` and `id` must each be either null or valid pointers to
/// NUL-terminated, UTF-8 C strings valid for the duration of this call.
/// The returned pointer (if non-null) must eventually be passed to
/// `dom_free_string` exactly once.
#[no_mangle]
pub unsafe extern "C" fn dom_get_element_by_id(
    input: *const c_char,
    id: *const c_char,
) -> *mut c_char {
    let html_input = match cstr_to_string(input) {
        Ok(s) => s,
        Err(e) => return string_to_c_char(error_json(e)),
    };
    let id_str = match cstr_to_string(id) {
        Ok(s) => s,
        Err(e) => return string_to_c_char(error_json(e)),
    };

    guarded_call(move || {
        let found = build_element_by_id(&html_input, &id_str);
        match found {
            Some(el) => to_json_string(&el),
            None => "null".to_string(),
        }
    })
}

/// Return all elements carrying `class` (matched as a whitespace-separated
/// token, matching standard HTML class semantics) as a JSON array.
///
/// # Safety
/// `input` and `class` must each be either null or valid pointers to
/// NUL-terminated, UTF-8 C strings valid for the duration of this call.
/// The returned pointer (if non-null) must eventually be passed to
/// `dom_free_string` exactly once.
#[no_mangle]
pub unsafe extern "C" fn dom_get_elements_by_class(
    input: *const c_char,
    class: *const c_char,
) -> *mut c_char {
    let html_input = match cstr_to_string(input) {
        Ok(s) => s,
        Err(e) => return string_to_c_char(error_json(e)),
    };
    let class_str = match cstr_to_string(class) {
        Ok(s) => s,
        Err(e) => return string_to_c_char(error_json(e)),
    };

    guarded_call(move || {
        let results = build_elements_by_class(&html_input, &class_str);
        to_json_string(&results)
    })
}

/// Extract every crawlable URL from `input` HTML (anchor hrefs, stylesheet
/// links, image/script/iframe/source src attributes, form actions) as a
/// deduplicated JSON array of strings.
///
/// # Safety
/// `input` must be either null or a valid pointer to a NUL-terminated,
/// UTF-8 C string valid for the duration of this call. The returned
/// pointer (if non-null) must eventually be passed to `dom_free_string`
/// exactly once.
#[no_mangle]
pub unsafe extern "C" fn dom_extract_urls(input: *const c_char) -> *mut c_char {
    let html_input = match cstr_to_string(input) {
        Ok(s) => s,
        Err(e) => return string_to_c_char(error_json(e)),
    };

    guarded_call(move || {
        let urls = build_extracted_urls(&html_input);
        to_json_string(&urls)
    })
}

/// Free a string previously returned by any `dom_*` function in this
/// module. Safe to call with a null pointer (no-op). Must not be called
/// twice on the same pointer, and must not be called on a pointer not
/// obtained from this module (double-free / foreign-free are both UB,
/// consistent with standard Rust FFI ownership rules).
///
/// # Safety
/// `ptr` must be either null, or a pointer previously returned by one of
/// this module's functions via `CString::into_raw`, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn dom_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // Reconstructing the CString and letting it drop deallocates the
    // buffer. catch_unwind here guards against any (theoretically
    // unreachable) panic during drop from propagating across FFI.
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        drop(CString::from_raw(ptr));
    }));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::time::Instant;

    fn cptr(s: &str) -> *const c_char {
        CString::new(s).unwrap().into_raw() as *const c_char
    }

    const SAMPLE: &str = r#"
        <html>
        <head>
            <title>  Kyvelix Test Page  </title>
            <meta name="description" content="A test page for the Kyvelix DOM parser.">
        </head>
        <body>
            <h1>Main Heading</h1>
            <h2>Sub Heading One</h2>
            <h2>Sub Heading Two</h2>
            <h3>Detail</h3>
            <p id="intro" class="lead highlighted">Hello   world.
            This   is   text.</p>
            <a href="https://example.com/page1" class="nav-link">Page 1</a>
            <a href="/relative/path" class="nav-link">Relative</a>
            <img src="/img/logo.png" alt="logo">
            <script>console.log("should not appear in text");</script>
            <style>.hidden { display: none; }</style>
        </body>
        </html>
    "#;

    #[test]
    fn test_dom_parse_basic_fields() {
        unsafe {
            let input = cptr(SAMPLE);
            let out = dom_parse(input);
            assert!(!out.is_null());
            let json_str = CStr::from_ptr(out).to_str().unwrap();
            let v: serde_json::Value = serde_json::from_str(json_str).unwrap();

            assert_eq!(v["title"], "Kyvelix Test Page");
            assert_eq!(
                v["meta_description"],
                "A test page for the Kyvelix DOM parser."
            );
            assert_eq!(v["headings"]["h1"][0], "Main Heading");
            assert_eq!(v["headings"]["h2"].as_array().unwrap().len(), 2);
            assert!(v["links"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l == "https://example.com/page1"));

            let text = v["text"].as_str().unwrap();
            assert!(text.contains("Hello world."));
            assert!(!text.contains("console.log"));
            assert!(!text.contains("display: none"));

            dom_free_string(out);
            let _ = CString::from_raw(input as *mut c_char);
        }
    }

    #[test]
    fn test_dom_query_selector() {
        unsafe {
            let input = cptr(SAMPLE);
            let selector = cptr("a.nav-link");
            let out = dom_query_selector(input, selector);
            let json_str = CStr::from_ptr(out).to_str().unwrap();
            let v: serde_json::Value = serde_json::from_str(json_str).unwrap();
            let arr = v.as_array().unwrap();
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0]["href"], "https://example.com/page1");

            dom_free_string(out);
            let _ = CString::from_raw(input as *mut c_char);
            let _ = CString::from_raw(selector as *mut c_char);
        }
    }

    #[test]
    fn test_dom_get_element_by_id_found_and_missing() {
        unsafe {
            let input = cptr(SAMPLE);
            let id = cptr("intro");
            let out = dom_get_element_by_id(input, id);
            let json_str = CStr::from_ptr(out).to_str().unwrap();
            let v: serde_json::Value = serde_json::from_str(json_str).unwrap();
            assert_eq!(v["tag"], "p");
            dom_free_string(out);
            let _ = CString::from_raw(id as *mut c_char);

            let input2 = cptr(SAMPLE);
            let missing_id = cptr("does-not-exist");
            let out2 = dom_get_element_by_id(input2, missing_id);
            let json_str2 = CStr::from_ptr(out2).to_str().unwrap();
            assert_eq!(json_str2, "null");
            dom_free_string(out2);

            let _ = CString::from_raw(input as *mut c_char);
            let _ = CString::from_raw(input2 as *mut c_char);
            let _ = CString::from_raw(missing_id as *mut c_char);
        }
    }

    #[test]
    fn test_dom_get_elements_by_class() {
        unsafe {
            let input = cptr(SAMPLE);
            let class = cptr("nav-link");
            let out = dom_get_elements_by_class(input, class);
            let json_str = CStr::from_ptr(out).to_str().unwrap();
            let v: serde_json::Value = serde_json::from_str(json_str).unwrap();
            assert_eq!(v.as_array().unwrap().len(), 2);
            dom_free_string(out);
            let _ = CString::from_raw(input as *mut c_char);
            let _ = CString::from_raw(class as *mut c_char);
        }
    }

    #[test]
    fn test_dom_extract_urls() {
        unsafe {
            let input = cptr(SAMPLE);
            let out = dom_extract_urls(input);
            let json_str = CStr::from_ptr(out).to_str().unwrap();
            let v: serde_json::Value = serde_json::from_str(json_str).unwrap();
            let arr = v.as_array().unwrap();
            assert!(arr.iter().any(|u| u == "/img/logo.png"));
            assert!(arr.iter().any(|u| u == "/relative/path"));
            dom_free_string(out);
            let _ = CString::from_raw(input as *mut c_char);
        }
    }

    #[test]
    fn test_null_pointer_safety() {
        unsafe {
            let out = dom_parse(std::ptr::null());
            assert!(!out.is_null());
            let json_str = CStr::from_ptr(out).to_str().unwrap();
            assert!(json_str.contains("error"));
            dom_free_string(out);

            // dom_free_string on null must be a safe no-op.
            dom_free_string(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_invalid_selector_returns_error_json() {
        unsafe {
            let input = cptr(SAMPLE);
            let bad_selector = cptr(":::not-a-selector:::");
            let out = dom_query_selector(input, bad_selector);
            let json_str = CStr::from_ptr(out).to_str().unwrap();
            assert!(json_str.contains("error"));
            dom_free_string(out);
            let _ = CString::from_raw(input as *mut c_char);
            let _ = CString::from_raw(bad_selector as *mut c_char);
        }
    }

    #[test]
    fn test_parse_performance_under_20ms() {
        unsafe {
            let input = cptr(SAMPLE);
            let start = Instant::now();
            let out = dom_parse(input);
            let elapsed = start.elapsed();
            dom_free_string(out);
            let _ = CString::from_raw(input as *mut c_char);
            assert!(
                elapsed.as_millis() < 20,
                "dom_parse took {:?}, exceeding 20ms budget",
                elapsed
            );
        }
    }
}