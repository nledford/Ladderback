//! Security response headers (technical design §10.1), on every response.
//!
//! With sign-in off the web UI is a local admin console that can set hooks,
//! so a page that frames it, or a browser that guesses what a response is,
//! can do harm even on loopback:
//!
//! - `Content-Security-Policy` lets the page run its own files and
//!   WebAssembly, and nothing else, and lets no site frame it;
//! - `X-Content-Type-Options: nosniff` makes a browser take a response for
//!   the type it says;
//! - `Referrer-Policy: no-referrer` keeps the UI's addresses, which name
//!   repositories and plans, out of any request a link or an asset makes;
//! - `Cache-Control: no-store` on every answer of the API, a refusal's
//!   included, so no caching proxy in front keeps one person's answer, a
//!   session's included, and gives it to another (#397).
//!
//! There is no `Strict-Transport-Security`: the daemon serves plain HTTP on
//! loopback, and a browser that kept the header would refuse plain HTTP to
//! whatever else later listens on that address.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS,
};
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};

/// The policy of a response that is not the entry page: the page's own files
/// and WebAssembly (`'wasm-unsafe-eval'` compiles the module, and allows no
/// `eval` of text), no framing, no `<base>`, and forms that stay on the
/// page's origin.
const POLICY: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// The directive of [`POLICY`] that a page's inline scripts are added to.
const SCRIPT_SRC: &str = "script-src 'self' 'wasm-unsafe-eval'";

/// The policy of an HTML page: [`POLICY`], plus the hash of each script the
/// page holds inline. Trunk writes the one that starts the application into
/// `index.html`, its file names carrying a content hash, so no fixed policy
/// could name it; the hash allows exactly that text and no other inline code.
pub(crate) fn page_policy(html: &[u8]) -> HeaderValue {
    let hashes: Vec<String> = inline_scripts(html)
        .map(|script| format!("'sha256-{}'", STANDARD.encode(Sha256::digest(script))))
        .collect();
    let policy = POLICY.replacen(
        SCRIPT_SRC,
        &format!("{SCRIPT_SRC} {}", hashes.join(" ")),
        usize::from(!hashes.is_empty()),
    );
    // Base64 and ASCII directives are always header text.
    HeaderValue::from_str(&policy).unwrap_or_else(|_| HeaderValue::from_static(POLICY))
}

/// The text of each `<script>` of `html` that has no `src`, byte for byte as
/// a browser hashes it. It reads the page the build wrote, not a page a
/// stranger sends, so it knows no more of HTML than that.
fn inline_scripts(html: &[u8]) -> impl Iterator<Item = &[u8]> {
    const OPEN: &[u8] = b"<script";
    const CLOSE: &[u8] = b"</script";
    let mut rest = html;
    std::iter::from_fn(move || {
        loop {
            let open = find_ignoring_case(rest, OPEN)?;
            let after = rest.get(open + OPEN.len()..)?;
            rest = after;
            // `<scripts>` isn't a script: the name ends at a space, `/` or `>`.
            if !after
                .first()
                .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
            {
                continue;
            }
            let tag_end = after.iter().position(|byte| *byte == b'>')?;
            let content = after.get(tag_end + 1..)?;
            let close = find_ignoring_case(content, CLOSE)?;
            rest = content.get(close..)?;
            if !has_src(after.get(..tag_end)?) {
                return content.get(..close);
            }
        }
    })
}

/// Whether the attributes of a tag include `src`.
fn has_src(tag: &[u8]) -> bool {
    tag.split(|byte| byte.is_ascii_whitespace())
        .any(|attribute| {
            attribute
                .get(..3)
                .is_some_and(|name| name.eq_ignore_ascii_case(b"src"))
                && matches!(attribute.get(3), None | Some(b'='))
        })
}

fn find_ignoring_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

/// Adds the headers to a response, whatever answered it. The entry page
/// carries its own policy, which this leaves as it is.
pub(crate) async fn add(request: Request, next: Next) -> Response {
    let api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    if api {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers
        .entry(CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static(POLICY));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Trunk writes into `index.html`: one inline module script, and
    /// scripts that name a file.
    const TRUNK_PAGE: &str = r#"<!doctype html><html><head>
<link rel="stylesheet" href="/style.css" integrity="sha384-x"/>
<script type="module">
import init, * as bindings from '/web-1f46a27bf13d55c3.js';
await init({ module_or_path: '/web-cf33d289e895797f_bg.wasm' });
</script>
<link rel="modulepreload" href="/web.js"></head><body></body></html>"#;

    fn page_policy_text(html: &str) -> String {
        page_policy(html.as_bytes()).to_str().unwrap().to_owned()
    }

    fn sha256_of(script: &str) -> String {
        format!("'sha256-{}'", STANDARD.encode(Sha256::digest(script)))
    }

    #[test]
    fn a_page_without_inline_scripts_gets_the_plain_policy() {
        for html in [
            "",
            "<title>x</title>",
            r#"<script src="/a.js"></script>"#,
            r#"<SCRIPT SRC=/a.js></SCRIPT>"#,
            r#"<script type="module" src='/a.js' defer></script>"#,
            "<scripts>alert(1)</scripts>",
        ] {
            assert_eq!(page_policy_text(html), POLICY, "{html:?}");
        }
    }

    #[test]
    fn the_inline_script_of_a_page_is_allowed_by_its_hash_and_nothing_else_is() {
        let script = "\nimport init, * as bindings from '/web-1f46a27bf13d55c3.js';\nawait init({ module_or_path: '/web-cf33d289e895797f_bg.wasm' });\n";
        let policy = page_policy_text(TRUNK_PAGE);
        assert_eq!(
            policy,
            POLICY.replace(
                "'wasm-unsafe-eval'",
                &format!("'wasm-unsafe-eval' {}", sha256_of(script))
            )
        );
        assert!(!policy.contains("unsafe-inline"));
    }

    #[test]
    fn each_inline_script_is_hashed_and_a_changed_script_changes_its_hash() {
        let two = page_policy_text("<script>a()</script><p><script type=module>b()</script>");
        assert!(
            two.contains(&format!("{} {}", sha256_of("a()"), sha256_of("b()"))),
            "{two}"
        );
        assert_ne!(
            page_policy_text("<script>a()</script>"),
            page_policy_text("<script>a() </script>")
        );
        // A script with a source is no inline script, even beside one that is.
        let mixed = page_policy_text(r#"<script src="/a.js"></script><script>b()</script>"#);
        assert!(mixed.contains(&sha256_of("b()")), "{mixed}");
        assert_eq!(mixed.matches("'sha256-").count(), 1, "{mixed}");
    }

    #[test]
    fn an_unfinished_script_is_not_hashed() {
        for html in ["<script>a()", "<script", "<script type=module"] {
            assert_eq!(page_policy_text(html), POLICY, "{html:?}");
        }
    }
}
