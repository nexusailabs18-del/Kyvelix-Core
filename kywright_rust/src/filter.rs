//! Streaming HTML filter.
//!
//! Design notes (read before trusting this in production):
//!
//! - This is NOT a full HTML parser. It is a lightweight tag-aware scanner
//!   that tracks enough state (inside <script>, inside <style>, inside a
//!   suppressed ad/tracking element) to make reasonable removal decisions
//!   without paying for a full DOM build. For guaranteed-correct parsing
//!   (malformed HTML, nested/foreign content, encoding edge cases), a real
//!   parser (html5ever) is still the right tool — that's `dom.rs`'s job.
//! - The filter is chunk-incremental: `StreamFilter::feed()` can be called
//!   repeatedly with arbitrary byte chunks (e.g. as they arrive from a
//!   network stream) and state persists correctly across chunk boundaries.
//!   This is the part a naive "line by line" filter gets wrong.
//! - Element removal (ads, tracking pixels, script/style/iframe) is done by
//!   depth-tracked tag matching, not regex-over-whole-document, so a single
//!   `<div class="ad-banner">...many lines...</div>` is removed correctly
//!   even if it spans chunk boundaries.
//! - PII redaction (email/phone) runs on the *retained* text after
//!   structural filtering, using regexes compiled once via `once_cell`.
//!   Phone regex is a best-effort heuristic (see module docs) — do not
//!   treat this as a compliance-grade PII scrubber.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::VecDeque;
use crate::brave_filter;

// ---------------------------------------------------------------------
// PII regexes — compiled once, reused across all filter instances.
// ---------------------------------------------------------------------

static EMAIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").unwrap()
});

// Heuristic only. Matches common US/international-ish patterns with
// separators. Will miss many valid formats and may false-positive on
// things like invoice numbers or IDs. Flagged in module docs above.
static PHONE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:\+?91[\s.-]?)?[6-9]\d{4}[\s.-]?\d{5}\b|\+?[1-9]\d{1,3}[\s.-]\d{3,4}[\s.-]\d{3,4}\b")
        .unwrap()
});

static DOB_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(19|20)\d{2}[-/.](0[1-9]|1[0-2])[-/.](0[1-9]|[12]\d|3[01])\b").unwrap()
});

static CUSTOMER_ID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bCUST-\d+\b").unwrap()
});

static GOVT_ID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b[A-Z]{2,}-\d{10,}\b").unwrap()
});

static TRACKING_PIXEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?is)<img\b[^>]*?\bwidth\s*=\s*["']?1["']?\b[^>]*?\bheight\s*=\s*["']?1["']?\b[^>]*?>|<img\b[^>]*?\bheight\s*=\s*["']?1["']?\b[^>]*?\bwidth\s*=\s*["']?1["']?\b[^>]*?>"#,
    )
    .unwrap()
});

static CLASS_OR_ID_ATTR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?is)\b(class|id)\s*=\s*["']([^"']*)["']"#).unwrap());

static AD_TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(^|[\s_-])(ad|ads|advert|advertisement|sponsored|tracking|gtm|doubleclick)([\s_-]|$)")
        .unwrap()
});

const STRIP_CONTAINER_TAGS: &[&str] = &["script", "style", "iframe", "noscript"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Passthrough,
    InStripTag { tag: &'static str, depth: u32 },
    InAdElement { tag: String, depth: u32 },
}

pub struct StreamFilter {
    mode: Mode,
    carry: String,
    out: String,
    _tag_stack_debug: VecDeque<String>,
}

impl Default for StreamFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamFilter {
    pub fn new() -> Self {
        StreamFilter {
            mode: Mode::Passthrough,
            carry: String::new(),
            out: String::new(),
            _tag_stack_debug: VecDeque::with_capacity(8),
        }
    }

    pub fn feed(&mut self, chunk: &str) {
        self.carry.push_str(chunk);
        self.process_buffer(false);
    }

    pub fn finish(&mut self) {
        self.process_buffer(true);
    }

    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.out)
    }

    fn process_buffer(&mut self, eof: bool) {
        loop {
            match self.mode {
                Mode::Passthrough => {
                    match self.carry.find('<') {
                        Some(lt_idx) => {
                            let (text, rest) = self.carry.split_at(lt_idx);
                            let redacted = redact_pii(text);
                            self.out.push_str(&redacted);
                            self.carry = rest.to_string();

                            match find_tag_end(&self.carry) {
                                Some(gt_idx) => {
                                    let tag_str = self.carry[..=gt_idx].to_string();
                                    self.carry = self.carry[gt_idx + 1..].to_string();
                                    self.handle_tag(&tag_str);
                                }
                                None => {
                                    if eof {
                                        let leftover = std::mem::take(&mut self.carry);
                                        self.out.push_str(&redact_pii(&leftover));
                                    }
                                    break;
                                }
                            }
                        }
                        None => {
                            if eof {
                                let leftover = std::mem::take(&mut self.carry);
                                self.out.push_str(&redact_pii(&leftover));
                            } else {
                                if !self.carry.is_empty() {
                                    let leftover = std::mem::take(&mut self.carry);
                                    self.out.push_str(&redact_pii(&leftover));
                                }
                            }
                            break;
                        }
                    }
                }

                Mode::InStripTag { tag, depth } => {
                    match find_next_tag_boundary(&self.carry, tag) {
                        Some(TagBoundary::Open(end)) => {
                            self.mode = Mode::InStripTag { tag, depth: depth + 1 };
                            self.carry = self.carry[end..].to_string();
                        }
                        Some(TagBoundary::Close(end)) => {
                            if depth <= 1 {
                                self.mode = Mode::Passthrough;
                            } else {
                                self.mode = Mode::InStripTag { tag, depth: depth - 1 };
                            }
                            self.carry = self.carry[end..].to_string();
                        }
                        None => {
                            if eof {
                                self.carry.clear();
                                self.mode = Mode::Passthrough;
                            }
                            break;
                        }
                    }
                }

                Mode::InAdElement { ref tag, depth } => {
                    let tag_owned = tag.clone();
                    match find_next_tag_boundary(&self.carry, &tag_owned) {
                        Some(TagBoundary::Open(end)) => {
                            self.mode = Mode::InAdElement { tag: tag_owned, depth: depth + 1 };
                            self.carry = self.carry[end..].to_string();
                        }
                        Some(TagBoundary::Close(end)) => {
                            if depth <= 1 {
                                self.mode = Mode::Passthrough;
                            } else {
                                self.mode = Mode::InAdElement { tag: tag_owned, depth: depth - 1 };
                            }
                            self.carry = self.carry[end..].to_string();
                        }
                        None => {
                            if eof {
                                self.carry.clear();
                                self.mode = Mode::Passthrough;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    fn handle_tag(&mut self, tag_str: &str) {
        let lower = tag_str.to_ascii_lowercase();

        if lower.starts_with("</") {
            self.out.push_str(tag_str);
            return;
        }

        if TRACKING_PIXEL_RE.is_match(tag_str) {
            return;
        }

        let tag_name = extract_tag_name(&lower);

        if let Some(name) = tag_name {
            if STRIP_CONTAINER_TAGS.contains(&name.as_str()) {
                let is_self_closing = tag_str.trim_end().ends_with("/>");
                if !is_self_closing {
                    let leaked: &'static str = match name.as_str() {
                        "script" => "script",
                        "style" => "style",
                        "iframe" => "iframe",
                        "noscript" => "noscript",
                        _ => unreachable!(),
                    };
                    self.mode = Mode::InStripTag { tag: leaked, depth: 1 };
                }
                return;
            }

            let is_ad = CLASS_OR_ID_ATTR_RE
                .captures_iter(tag_str)
                .any(|cap| AD_TOKEN_RE.is_match(&cap[2]));

            if is_ad {
                let is_self_closing = tag_str.trim_end().ends_with("/>");
                if !is_self_closing {
                    self.mode = Mode::InAdElement { tag: name, depth: 1 };
                }
                return;
            }
        }

        self.out.push_str(tag_str);
    }
}

enum TagBoundary {
    Open(usize),
    Close(usize),
}

fn find_next_tag_boundary(buf: &str, tag: &str) -> Option<TagBoundary> {
    let lower = buf.to_ascii_lowercase();
    let open_needle = format!("<{}", tag);
    let close_needle = format!("</{}", tag);

    let open_pos = lower.find(&open_needle).and_then(|p| {
        let after = p + open_needle.len();
        match lower.as_bytes().get(after) {
            Some(b) if *b == b' ' || *b == b'>' || *b == b'/' || *b == b'\t' || *b == b'\n' || *b == b'\r' => {
                find_tag_end(&buf[p..]).map(|rel_end| p + rel_end + 1)
            }
            _ => None,
        }
    });

    let close_pos = lower.find(&close_needle).and_then(|p| {
        find_tag_end(&buf[p..]).map(|rel_end| p + rel_end + 1)
    });

    match (open_pos, close_pos) {
        (Some(o), Some(c)) => {
            if o < c {
                Some(TagBoundary::Open(o))
            } else {
                Some(TagBoundary::Close(c))
            }
        }
        (Some(o), None) => Some(TagBoundary::Open(o)),
        (None, Some(c)) => Some(TagBoundary::Close(c)),
        (None, None) => None,
    }
}

fn find_tag_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] != b'<' {
        return None;
    }
    let mut in_squote = false;
    let mut in_dquote = false;
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if !in_squote => in_dquote = !in_dquote,
            b'\'' if !in_dquote => in_squote = !in_squote,
            b'>' if !in_squote && !in_dquote => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn extract_tag_name(lower_tag: &str) -> Option<String> {
    let s = lower_tag.trim_start_matches('<');
    let end = s
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(s.len());
    if end == 0 {
        None
    } else {
        Some(s[..end].to_string())
    }
}

fn redact_pii(text: &str) -> String {
    // Protect IP addresses from phone redaction
    let ip_re = regex::Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").unwrap();
    let mut protected = String::new();
    let mut last = 0;
    let mut ips = Vec::new();

    for m in ip_re.find_iter(text) {
        protected.push_str(&text[last..m.start()]);
        let placeholder = format!("__KYIP{}__", ips.len());
        ips.push(m.as_str().to_string());
        protected.push_str(&placeholder);
        last = m.end();
    }
    protected.push_str(&text[last..]);

    let redacted_email = EMAIL_RE.replace_all(&protected, "[REDACTED_EMAIL]");
    let redacted_phone = PHONE_RE.replace_all(&redacted_email, "[REDACTED_PHONE]");
    let redacted_dob = DOB_RE.replace_all(&redacted_phone, "[REDACTED_DOB]");
    let redacted_cust = CUSTOMER_ID_RE.replace_all(&redacted_dob, "[REDACTED_CUSTOMER_ID]");
    let redacted_govt = GOVT_ID_RE.replace_all(&redacted_cust, "[REDACTED_GOVT_ID]");

    let mut result = redacted_govt.into_owned();
    for (i, ip) in ips.iter().enumerate() {
        let placeholder = format!("__KYIP{}__", i);
        result = result.replace(&placeholder, ip);
    }

    result
}


fn cleanup_filtered(html: String) -> String {
    let iframe_re = Regex::new(r"(?i)</iframe\s*>").unwrap();
    let cleaned = iframe_re.replace_all(&html, "").to_string();

    let newline_re = Regex::new(r"\n\s*\n").unwrap();
    let collapsed = newline_re.replace_all(&cleaned, "\n").to_string();

    collapsed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn filter_html_str(input: &str) -> String {
    let pre_filtered = brave_filter::brave_filter_html(input);
    let mut f = StreamFilter::new();
    f.feed(&pre_filtered);
    f.finish();
    cleanup_filtered(f.take_output())
}

pub fn filter_html_str_with_url(input: &str, page_url: &str) -> String {
    let pre_filtered = brave_filter::brave_filter_html_with_context(input, Some(page_url));
    let mut f = StreamFilter::new();
    f.feed(&pre_filtered);
    f.finish();
    cleanup_filtered(f.take_output())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_tags() {
        let input = r#"<p>hello</p><script>alert('x')</script><p>world</p>"#;
        let out = filter_html_str(input);
        assert!(!out.contains("script"));
        assert!(!out.contains("alert"));
        assert!(out.contains("hello"));
        assert!(out.contains("world"));
    }

    #[test]
    fn strips_multiline_script() {
        let input = "<p>a</p><script>\nfunction f() {\n  return 1;\n}\n</script><p>b</p>";
        let out = filter_html_str(input);
        assert!(!out.contains("function"));
        assert!(out.contains("<p>a</p>"));
        assert!(out.contains("<p>b</p>"));
    }

    #[test]
    fn strips_style_and_iframe() {
        let input = r#"<style>.x{color:red}</style><iframe src="evil.com"></iframe><p>ok</p>"#;
        let out = filter_html_str(input);
        assert!(!out.contains("color:red"));
        assert!(!out.contains("iframe"));
        assert!(out.contains("<p>ok</p>"));
    }

    #[test]
    fn removes_ad_by_class() {
        let input = r#"<div class="ad-banner"><p>buy now</p></div><p>real content</p>"#;
        let out = filter_html_str(input);
        assert!(!out.contains("buy now"));
        assert!(out.contains("real content"));
    }

    #[test]
    fn does_not_false_positive_on_gradient_or_header() {
        let input = r#"<div class="gradient-header"><p>legit</p></div>"#;
        let out = filter_html_str(input);
        assert!(out.contains("legit"));
    }

    #[test]
    fn removes_nested_ad_div_correctly() {
        let input = r#"<div class="ad-slot"><div class="ad-slot">nested</div>still ad</div><p>keep</p>"#;
        let out = filter_html_str(input);
        assert!(!out.contains("nested"));
        assert!(!out.contains("still ad"));
        assert!(out.contains("keep"));
    }

    #[test]
    fn removes_tracking_pixel() {
        let input = r#"<p>a</p><img src="x.gif" width="1" height="1"><p>b</p>"#;
        let out = filter_html_str(input);
        assert!(!out.contains("x.gif"));
        assert!(out.contains("<p>a</p>"));
        assert!(out.contains("<p>b</p>"));
    }

    #[test]
    fn redacts_email() {
        let input = r#"<p>contact me at john.doe@example.com please</p>"#;
        let out = filter_html_str(input);
        assert!(!out.contains("john.doe@example.com"));
        assert!(out.contains("[REDACTED_EMAIL]"));
    }

    #[test]
    fn handles_chunk_boundary_mid_tag() {
        let mut f = StreamFilter::new();
        f.feed("<scr");
        f.feed("ipt>var x = 1;</scr");
        f.feed("ipt><p>after</p>");
        f.finish();
        let out = f.take_output();
        assert!(!out.contains("var x"));
        assert!(out.contains("after"));
    }

    #[test]
    fn handles_chunk_boundary_mid_text() {
        let mut f = StreamFilter::new();
        f.feed("<p>hello wor");
        f.feed("ld</p>");
        f.finish();
        let out = f.take_output();
        assert_eq!(out, "<p>hello world</p>");
    }

    #[test]
    fn preserves_ordinary_content_unchanged() {
        let input = r#"<html><head><title>T</title></head><body><h1>Hi</h1></body></html>"#;
        let out = filter_html_str(input);
        assert_eq!(out, input);
    }
}
