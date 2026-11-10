//! Text of a profile that has to stay text in the config.
//!
//! The config's repository fields are templates (technical design §10.2): a
//! `${VAR}` in one is replaced by the daemon's environment, and
//! `${file:/path}` by the contents of a file. A profile is written by whoever
//! wrote it, so `repository = "s3:${file:/etc/passwd}"` must not become a
//! reference when it is imported: what the profile says is escaped, and only
//! the references this adapter makes itself (for rustic's own variables) are
//! not.

/// `text` written so that the template grammar reads it back as itself: every
/// `${` is `$${`, which the grammar reads as a literal `${`. Any other text
/// has no meaning there.
pub(crate) fn text(text: &str) -> String {
    text.replace("${", "$${")
}

#[cfg(test)]
mod tests {
    use domain::secrets::{Part, Template};

    use super::*;

    fn read_back(text: &str) -> String {
        match Template::parse(text).expect("a template").parts() {
            [] => String::new(),
            [Part::Text(text)] => text.clone(),
            other => panic!("{text:?} is not plain text: {other:?}"),
        }
    }

    #[test]
    fn what_the_profile_wrote_reads_back_as_the_same_text() {
        for written in [
            "",
            "plain",
            "$",
            "$$",
            "${",
            "${VAR}",
            "$${VAR}",
            "$$${VAR}",
            "$${",
            "a${b}c${d",
            "${file:/etc/passwd}",
            "100%$",
            "${${",
            "}${",
            "pässwörd ${A} ✓",
        ] {
            assert_eq!(read_back(&text(written)), written, "{written:?}");
        }
    }
}
