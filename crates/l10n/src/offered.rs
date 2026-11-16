//! How `offered-locales.txt` is read. Shared by `build.rs`, which generates
//! `catalog::LISTED` with it, and, under `cfg(test)`, by the library, because
//! `cargo test` can't reach a build script's own functions.

/// The list of the languages a release offers, beside `Cargo.toml`.
pub(crate) const FILE: &str = "offered-locales.txt";

/// The source locale, which every chain of locales ends in.
const SOURCE_TAG: &str = "en-US";

/// The tags `text` lists, one a line; a blank line and one that starts with
/// `#` say nothing. Fails, naming the line, on a tag none of `catalogs` is,
/// and on a tag listed twice; fails on a list without `en-US`.
pub(crate) fn listed(text: &str, catalogs: &[&str]) -> Result<Vec<String>, String> {
    let mut tags: Vec<String> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let tag = line.trim();
        if tag.is_empty() || tag.starts_with('#') {
            continue;
        }
        let number = index + 1;
        if !catalogs.contains(&tag) {
            return Err(format!(
                "{FILE} line {number}: {tag:?} has no catalog: no folder locales/{tag}"
            ));
        }
        if tags.iter().any(|listed| listed == tag) {
            return Err(format!("{FILE} line {number}: {tag} is listed twice"));
        }
        tags.push(tag.to_owned());
    }
    if !tags.iter().any(|tag| tag == SOURCE_TAG) {
        return Err(format!("{FILE} must list {SOURCE_TAG}, the source locale"));
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOGS: &[&str] = &["en-US", "de", "ja"];

    #[test]
    fn the_listed_tags_come_in_order_without_comments_and_blank_lines() {
        let text = "# The languages\n\n  en-US  \nja\n# de\n";
        assert_eq!(listed(text, CATALOGS).unwrap(), ["en-US", "ja"]);
    }

    #[test]
    fn a_tag_with_no_catalog_is_refused_naming_its_line() {
        let error = listed("en-US\n\nfr\n", CATALOGS).unwrap_err();
        assert_eq!(
            error,
            "offered-locales.txt line 3: \"fr\" has no catalog: no folder locales/fr"
        );
    }

    #[test]
    fn a_tag_listed_twice_is_refused_naming_the_second_line() {
        let error = listed("en-US\nde\nde\n", CATALOGS).unwrap_err();
        assert_eq!(error, "offered-locales.txt line 3: de is listed twice");
    }

    #[test]
    fn a_list_without_american_english_is_refused() {
        let error = listed("de\n", CATALOGS).unwrap_err();
        assert_eq!(
            error,
            "offered-locales.txt must list en-US, the source locale"
        );
        assert!(listed("# en-US\n", CATALOGS).is_err());
    }
}
