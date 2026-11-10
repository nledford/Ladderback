//! The browser has no time-zone database on disk, so the web build resolves a
//! zone from the copy `jiff` carries (technical design §3.2). `just test-wasm`
//! runs this under Node, which has no `/usr/share/zoneinfo` for the test to
//! find either.

use jiff::Timestamp;
use jiff::tz::{Offset, TimeZone};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn a_zone_resolves_in_the_browser_build_with_its_daylight_saving_rules() {
    let zone = TimeZone::get("America/New_York").unwrap();
    let winter: Timestamp = "2026-11-08T13:30:45Z".parse().unwrap();
    let summer: Timestamp = "2026-07-08T13:30:45Z".parse().unwrap();
    assert_eq!(zone.to_offset(winter), Offset::constant(-5));
    assert_eq!(zone.to_offset(summer), Offset::constant(-4));
}
