//! `${…}` in repository fields (technical design §10.2): parsing only. The
//! use case that resolves the references and joins the parts owns the rest.
//!
//! The grammar is deliberately small. `${VAR}` is an environment variable and
//! `${file:/absolute/path}` a file's contents; `$${` is a literal `${`.
//! There are no defaults, no nesting, and what a reference expands to is
//! never scanned for references again.

use super::source::{FileRead, SecretRef};
use super::{MAX_REFERENCES, MAX_SECRET_BYTES};

/// What a `${…}` stands for.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reference {
    /// `${VAR}`: an environment variable of the daemon.
    Env(String),
    /// `${file:/path}`: the contents of a file, minus one trailing newline.
    File(String),
}

impl Reference {
    /// The source that provides the value.
    pub fn secret_ref(&self) -> SecretRef {
        todo!("the source that provides the value")
    }
}

/// One piece of a template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    /// Text to keep as it is. Escapes are already resolved: `$${` is `${`.
    Text(String),
    Reference(Reference),
}

/// A repository field, parsed. Parsing never reads the environment or a
/// file, so it is how a config is checked when it loads; the references are
/// resolved when a job starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    parts: Vec<Part>,
}

/// Why a field isn't a valid template. `at` is the byte offset of the `${`
/// at fault. An error never holds the text: a field may be an inline secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateError {
    /// Longer than [`MAX_SECRET_BYTES`].
    TooLong { limit: usize },
    /// More than [`MAX_REFERENCES`] references.
    TooManyReferences { limit: usize },
    /// A `${` with no `}` after it.
    Unterminated { at: usize },
    /// `${}`.
    EmptyReference { at: usize },
    /// `${` followed by something that is neither a variable name
    /// (`[A-Za-z_][A-Za-z0-9_]*`) nor `file:`.
    InvalidName { at: usize },
    /// `${file:}`.
    EmptyPath { at: usize },
    /// `${file:` and a path that isn't absolute.
    RelativePath { at: usize },
    /// A `${` inside a reference.
    NestedReference { at: usize },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for TemplateError {}

impl Template {
    pub fn parse(_text: &str) -> Result<Self, TemplateError> {
        todo!("the parts of the text, or where it is malformed")
    }

    pub fn parts(&self) -> &[Part] {
        todo!("the parts")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Part {
        Part::Text(text.to_owned())
    }

    fn env(name: &str) -> Part {
        Part::Reference(Reference::Env(name.to_owned()))
    }

    fn file(path: &str) -> Part {
        Part::Reference(Reference::File(path.to_owned()))
    }

    fn parts(input: &str) -> Vec<Part> {
        Template::parse(input).unwrap().parts().to_vec()
    }

    fn error(input: &str) -> TemplateError {
        Template::parse(input).unwrap_err()
    }

    #[test]
    fn text_without_a_reference_is_kept_as_it_is() {
        assert_eq!(parts("opendal:b2"), [text("opendal:b2")]);
        assert_eq!(parts(""), Vec::<Part>::new());
        // A dollar sign or a brace alone is not a reference.
        for plain in ["a$b", "$", "$$", "{VAR}", "}", "$ {VAR}", "100%$"] {
            assert_eq!(parts(plain), [text(plain)], "{plain}");
        }
        // Not ASCII.
        assert_eq!(
            parts("pässwörd ${A} ✓"),
            [text("pässwörd "), env("A"), text(" ✓")]
        );
    }

    #[test]
    fn a_variable_is_a_reference() {
        assert_eq!(parts("${B2_KEY_ID}"), [env("B2_KEY_ID")]);
        assert_eq!(parts("${_x1}"), [env("_x1")]);
        assert_eq!(parts("${a}"), [env("a")]);
        // A variable that happens to be called `file`.
        assert_eq!(parts("${file}"), [env("file")]);
    }

    #[test]
    fn a_file_is_a_reference() {
        assert_eq!(
            parts("${file:/run/secrets/b2_app_key}"),
            [file("/run/secrets/b2_app_key")]
        );
        // The path may hold anything but a closing brace.
        assert_eq!(
            parts("${file:/home/demo/my secrets/key:1.txt}"),
            [file("/home/demo/my secrets/key:1.txt")]
        );
        assert_eq!(parts("${file:/}"), [file("/")]);
    }

    #[test]
    fn a_reference_can_sit_inside_other_text_and_several_can_share_a_field() {
        assert_eq!(
            parts("https://${S3_HOST}:9000/${file:/run/secrets/bucket}/x"),
            [
                text("https://"),
                env("S3_HOST"),
                text(":9000/"),
                file("/run/secrets/bucket"),
                text("/x"),
            ]
        );
        assert_eq!(parts("${A}${B}"), [env("A"), env("B")]);
        assert_eq!(parts("${A}${A}"), [env("A"), env("A")]);
    }

    #[test]
    fn a_doubled_dollar_keeps_the_reference_literal() {
        assert_eq!(parts("$${VAR}"), [text("${VAR}")]);
        assert_eq!(parts("a$${b}c"), [text("a${b}c")]);
        assert_eq!(parts("$${file:/x}"), [text("${file:/x}")]);
        // Nothing needs to follow the brace.
        assert_eq!(parts("$${"), [text("${")]);
        assert_eq!(parts("pa$${"), [text("pa${")]);
    }

    #[test]
    fn an_escape_and_a_reference_can_share_a_field() {
        assert_eq!(parts("$${A}${B}"), [text("${A}"), env("B")]);
        assert_eq!(parts("${A}$${B}"), [env("A"), text("${B}")]);
        // The escape is `$${`, read from the left: a lone `$`, then `$${`.
        assert_eq!(parts("$$${A}"), [text("$${A}")]);
        assert_eq!(parts("$$$${A}"), [text("$$${A}")]);
        // A dollar before a reference is just a dollar.
        assert_eq!(parts("$ ${A}"), [text("$ "), env("A")]);
    }

    #[test]
    fn the_references_of_a_template_come_in_order() {
        let template = Template::parse("${A}-${file:/b}-${A}").unwrap();
        let references: Vec<&Reference> = template
            .parts()
            .iter()
            .filter_map(|part| match part {
                Part::Reference(reference) => Some(reference),
                Part::Text(_) => None,
            })
            .collect();
        assert_eq!(
            references,
            [
                &Reference::Env("A".to_owned()),
                &Reference::File("/b".to_owned()),
                &Reference::Env("A".to_owned()),
            ]
        );
    }

    #[test]
    fn a_reference_that_is_not_closed_is_an_error_at_its_start() {
        assert_eq!(error("${A"), TemplateError::Unterminated { at: 0 });
        assert_eq!(error("ab ${A"), TemplateError::Unterminated { at: 3 });
        assert_eq!(error("${"), TemplateError::Unterminated { at: 0 });
        assert_eq!(
            error("${A} and ${file:/x"),
            TemplateError::Unterminated { at: 9 }
        );
    }

    #[test]
    fn an_empty_reference_is_an_error() {
        assert_eq!(error("${}"), TemplateError::EmptyReference { at: 0 });
        assert_eq!(error("xx${}"), TemplateError::EmptyReference { at: 2 });
        assert_eq!(error("${file:}"), TemplateError::EmptyPath { at: 0 });
    }

    #[test]
    fn a_name_that_is_not_a_variable_name_is_an_error() {
        for body in [
            "A B",
            " A",
            "A ",
            "1A",
            "A-B",
            "A.B",
            "A:-default",
            "FILE:/x",
            "A=1",
            "ä",
            "A$",
        ] {
            assert_eq!(
                error(&format!("x${{{body}}}")),
                TemplateError::InvalidName { at: 1 },
                "{body}"
            );
        }
    }

    #[test]
    fn a_file_path_must_be_absolute() {
        for path in ["relative", "./x", "../x", "~/x", " /x"] {
            assert_eq!(
                error(&format!("${{file:{path}}}")),
                TemplateError::RelativePath { at: 0 },
                "{path}"
            );
        }
    }

    #[test]
    fn references_do_not_nest() {
        assert_eq!(
            error("${file:/a/${B}}"),
            TemplateError::NestedReference { at: 0 }
        );
        assert_eq!(error("${${B}}"), TemplateError::NestedReference { at: 0 });
        assert_eq!(error("ab${A${B}"), TemplateError::NestedReference { at: 2 });
        assert_eq!(
            error("${file:/a/$${B}}"),
            TemplateError::NestedReference { at: 0 }
        );
    }

    #[test]
    fn a_closing_brace_after_a_reference_is_text() {
        assert_eq!(parts("${A}}"), [env("A"), text("}")]);
    }

    #[test]
    fn a_field_is_limited_to_the_secret_size() {
        let at_the_limit = "a".repeat(MAX_SECRET_BYTES);
        assert_eq!(parts(&at_the_limit), [text(&at_the_limit)]);
        assert_eq!(
            error(&format!("{at_the_limit}a")),
            TemplateError::TooLong {
                limit: MAX_SECRET_BYTES
            }
        );
        // The limit counts bytes, not characters.
        let wide = "é".repeat(MAX_SECRET_BYTES / 2 + 1);
        assert_eq!(
            error(&wide),
            TemplateError::TooLong {
                limit: MAX_SECRET_BYTES
            }
        );
    }

    #[test]
    fn a_field_may_hold_only_so_many_references() {
        let at_the_limit = "${A}".repeat(MAX_REFERENCES);
        assert_eq!(parts(&at_the_limit).len(), MAX_REFERENCES);
        assert_eq!(
            error(&format!("{at_the_limit}${{A}}")),
            TemplateError::TooManyReferences {
                limit: MAX_REFERENCES
            }
        );
        // Escapes are not references.
        let escapes = "$${A}".repeat(MAX_REFERENCES + 1);
        assert_eq!(parts(&escapes), [text(&"${A}".repeat(MAX_REFERENCES + 1))]);
    }

    #[test]
    fn an_error_never_holds_the_text_it_is_about() {
        let debug = format!("{:?}", error("hunter2${"));
        assert!(!debug.contains("hunter2"), "{debug}");
        let debug = format!("{:?}", error("hunter2${A B}"));
        assert!(!debug.contains("hunter2"), "{debug}");
    }

    #[test]
    fn a_reference_is_read_by_the_source_that_provides_it() {
        assert_eq!(
            Reference::Env("A".to_owned()).secret_ref(),
            SecretRef::Env("A".to_owned())
        );
        assert_eq!(
            Reference::File("/x".to_owned()).secret_ref(),
            SecretRef::File {
                path: "/x".to_owned(),
                read: FileRead::Whole,
            }
        );
    }
}
