use axum::http::header::ACCEPT_ENCODING;
use axum::http::{HeaderMap, HeaderValue};

use super::Encoding::{Brotli, Gzip};
use super::*;

/// What a request with these `Accept-Encoding` lines prefers, in order.
fn preferred_for(lines: &[&str]) -> Vec<Encoding> {
    let mut headers = HeaderMap::new();
    for line in lines {
        headers.append(ACCEPT_ENCODING, HeaderValue::from_str(line).unwrap());
    }
    preferred(&headers).into_iter().flatten().collect()
}

#[test]
fn a_request_that_names_one_coding_gets_that_one() {
    assert_eq!(preferred_for(&["br"]), [Brotli]);
    assert_eq!(preferred_for(&["gzip"]), [Gzip]);
}

#[test]
fn a_request_without_the_header_or_with_none_of_the_codings_gets_neither() {
    assert_eq!(preferred_for(&[]), []);
    assert_eq!(preferred_for(&[""]), []);
    assert_eq!(preferred_for(&["identity"]), []);
    assert_eq!(preferred_for(&["deflate, zstd"]), []);
}

#[test]
fn brotli_comes_first_when_both_weigh_the_same() {
    assert_eq!(preferred_for(&["gzip, deflate, br"]), [Brotli, Gzip]);
    assert_eq!(preferred_for(&["gzip;q=0.5, br;q=0.5"]), [Brotli, Gzip]);
}

#[test]
fn the_heavier_coding_comes_first() {
    assert_eq!(preferred_for(&["br;q=0.5, gzip"]), [Gzip, Brotli]);
    assert_eq!(preferred_for(&["gzip;q=0.001, br;q=0.002"]), [Brotli, Gzip]);
    assert_eq!(preferred_for(&["br;q=0.9, gzip;q=1.000"]), [Gzip, Brotli]);
}

#[test]
fn a_coding_weighed_zero_is_refused() {
    assert_eq!(preferred_for(&["br;q=0, gzip"]), [Gzip]);
    assert_eq!(preferred_for(&["br, gzip;q=0.000"]), [Brotli]);
}

#[test]
fn codings_and_the_weight_s_name_are_read_regardless_of_case() {
    assert_eq!(preferred_for(&["BR, GZip"]), [Brotli, Gzip]);
    assert_eq!(preferred_for(&["br;Q=0.4, gzip;q=0.8"]), [Gzip, Brotli]);
}

#[test]
fn a_star_stands_for_each_coding_the_request_does_not_name() {
    assert_eq!(preferred_for(&["*"]), [Brotli, Gzip]);
    assert_eq!(preferred_for(&["*;q=0"]), []);
    assert_eq!(preferred_for(&["*, br;q=0"]), [Gzip]);
    assert_eq!(preferred_for(&["gzip;q=0.9, *;q=0.5"]), [Gzip, Brotli]);
    assert_eq!(preferred_for(&["br;q=0.2, *"]), [Gzip, Brotli]);
}

#[test]
fn x_gzip_is_gzip() {
    assert_eq!(preferred_for(&["x-gzip"]), [Gzip]);
}

#[test]
fn a_member_whose_weight_is_not_a_weight_is_left_out() {
    for weight in ["2", "1.001", "0.0001", "abc", "", "-1", "0.5.0", " 0.5"] {
        assert_eq!(
            preferred_for(&[&format!("br;q={weight}, gzip")]),
            [Gzip],
            "q={weight}"
        );
    }
}

#[test]
fn spaces_around_members_and_parameters_are_allowed() {
    assert_eq!(
        preferred_for(&[" br ; q=0.5 ,gzip ;level=9"]),
        [Gzip, Brotli]
    );
    assert_eq!(preferred_for(&[",, br ,"]), [Brotli]);
}

#[test]
fn every_line_of_the_header_counts() {
    assert_eq!(preferred_for(&["gzip", "br"]), [Brotli, Gzip]);
    assert_eq!(preferred_for(&["gzip", "br;q=0"]), [Gzip]);
}
