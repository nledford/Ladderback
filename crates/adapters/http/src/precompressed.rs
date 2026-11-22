//! The precompressed copies of the web UI's files (technical design §10.1):
//! which one a request's `Accept-Encoding` lets the daemon send. A release
//! build of the UI keeps a Brotli and a gzip copy beside each file that
//! compression makes smaller (`scripts/precompress.ts`); nothing is
//! compressed while a request waits, and no answer of the API ever is.

use axum::http::HeaderMap;
use axum::http::header::ACCEPT_ENCODING;

/// A content coding the web UI's build keeps a copy of a file in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    Brotli,
    Gzip,
}

impl Encoding {
    /// The coding's name in `Content-Encoding`.
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Brotli => "br",
            Self::Gzip => "gzip",
        }
    }

    /// What the copy's name adds to the file's.
    pub(crate) const fn suffix(self) -> &'static str {
        match self {
            Self::Brotli => ".br",
            Self::Gzip => ".gz",
        }
    }
}

/// The highest weight a coding can have, in thousandths.
const FULL: u16 = 1000;

/// The codings the request accepts, the one it prefers first: by their
/// weights in `Accept-Encoding` (RFC 9110 §12.5.3), Brotli first when both
/// weigh the same, as it makes the smaller copy. A coding the request
/// doesn't name takes the weight of `*`, a coding weighed 0 is refused, and
/// a request without the header accepts neither.
pub(crate) fn preferred(headers: &HeaderMap) -> [Option<Encoding>; 2] {
    let mut brotli = None;
    let mut gzip = None;
    let mut any = None;
    for (coding, weight) in headers
        .get_all(ACCEPT_ENCODING)
        .iter()
        .filter_map(|line| line.to_str().ok())
        .flat_map(|line| line.split(','))
        .filter_map(member)
    {
        let slot = if coding.eq_ignore_ascii_case("br") {
            &mut brotli
        } else if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
            &mut gzip
        } else if coding == "*" {
            &mut any
        } else {
            continue;
        };
        // A coding named twice takes its heavier weight.
        *slot = Some(slot.map_or(weight, |before: u16| before.max(weight)));
    }
    let brotli = brotli.or(any).unwrap_or(0);
    let gzip = gzip.or(any).unwrap_or(0);
    let order = if gzip > brotli {
        [(Encoding::Gzip, gzip), (Encoding::Brotli, brotli)]
    } else {
        [(Encoding::Brotli, brotli), (Encoding::Gzip, gzip)]
    };
    order.map(|(encoding, weight)| (weight > 0).then_some(encoding))
}

/// One member of the list: its coding and its weight in thousandths. `None`
/// for an empty member and for one whose weight is not a weight.
fn member(text: &str) -> Option<(&str, u16)> {
    let mut parts = text.split(';');
    let coding = parts.next()?.trim();
    if coding.is_empty() {
        return None;
    }
    let mut weight = FULL;
    for parameter in parts {
        let Some((name, value)) = parameter.trim().split_once('=') else {
            continue;
        };
        if name.eq_ignore_ascii_case("q") {
            weight = qvalue(value)?;
        }
    }
    Some((coding, weight))
}

/// A weight, `0` to `1` with at most three decimals (RFC 9110 §12.4.2), in
/// thousandths.
fn qvalue(text: &str) -> Option<u16> {
    let (whole, decimals) = text.split_once('.').unwrap_or((text, ""));
    if decimals.len() > 3 || !decimals.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut thousandths: u16 = 0;
    for position in 0..3 {
        let digit = decimals
            .as_bytes()
            .get(position)
            .map_or(0, |byte| byte - b'0');
        thousandths = thousandths * 10 + u16::from(digit);
    }
    match whole {
        "0" => Some(thousandths),
        "1" if thousandths == 0 => Some(FULL),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
