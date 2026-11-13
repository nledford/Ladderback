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
        match self {
            Self::Env(name) => SecretRef::Env(name.clone()),
            Self::File(path) => SecretRef::File {
                path: path.clone(),
                read: FileRead::Whole,
            },
        }
    }

    /// How many bytes the reference is written in: `${VAR}` or
    /// `${file:/path}`.
    pub(super) fn written_len(&self) -> usize {
        match self {
            Self::Env(name) => name.len().saturating_add("${}".len()),
            Self::File(path) => path.len().saturating_add("${file:}".len()),
        }
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

/// Whether `text` is a valid template with a `${…}` reference in it. A text
/// that is not a valid template has none: it is refused when a job resolves
/// it, and until then it is text as written.
pub fn has_reference(text: &str) -> bool {
    Template::parse(text).is_ok_and(|template| {
        template
            .parts()
            .iter()
            .any(|part| matches!(part, Part::Reference(_)))
    })
}

/// Whether `text` is a valid template made of `${…}` references alone, with
/// no text written around them: a value that says where its secrets are
/// and holds none, so a page may show it (I4).
pub fn only_references(text: &str) -> bool {
    Template::parse(text).is_ok_and(|template| {
        !template.parts().is_empty()
            && template
                .parts()
                .iter()
                .all(|part| matches!(part, Part::Reference(_)))
    })
}

impl Template {
    pub fn parse(text: &str) -> Result<Self, TemplateError> {
        if text.len() > MAX_SECRET_BYTES {
            return Err(TemplateError::TooLong {
                limit: MAX_SECRET_BYTES,
            });
        }
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut references = 0;
        let mut rest = text;
        while !rest.is_empty() {
            let at = text.len() - rest.len();
            if let Some(after) = rest.strip_prefix("$${") {
                literal.push_str("${");
                rest = after;
            } else if let Some(after) = rest.strip_prefix("${") {
                let (body, after) = after
                    .split_once('}')
                    .ok_or(TemplateError::Unterminated { at })?;
                let reference = parse_reference(body, at)?;
                references += 1;
                if references > MAX_REFERENCES {
                    return Err(TemplateError::TooManyReferences {
                        limit: MAX_REFERENCES,
                    });
                }
                if !literal.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut literal)));
                }
                parts.push(Part::Reference(reference));
                rest = after;
            } else {
                let mut chars = rest.chars();
                literal.extend(chars.next());
                rest = chars.as_str();
            }
        }
        if !literal.is_empty() {
            parts.push(Part::Text(literal));
        }
        Ok(Self { parts })
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }
}

/// The reference a `${…}` holds; `body` is what is between the braces.
fn parse_reference(body: &str, at: usize) -> Result<Reference, TemplateError> {
    if body.contains("${") {
        return Err(TemplateError::NestedReference { at });
    }
    if let Some(path) = body.strip_prefix("file:") {
        if path.is_empty() {
            return Err(TemplateError::EmptyPath { at });
        }
        if !path.starts_with('/') {
            return Err(TemplateError::RelativePath { at });
        }
        return Ok(Reference::File(path.to_owned()));
    }
    if body.is_empty() {
        return Err(TemplateError::EmptyReference { at });
    }
    if is_variable_name(body) {
        Ok(Reference::Env(body.to_owned()))
    } else {
        Err(TemplateError::InvalidName { at })
    }
}

/// `[A-Za-z_][A-Za-z0-9_]*`: what a shell allows in a variable name.
fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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

    #[test]
    fn only_a_value_of_references_alone_is_one_a_page_may_show() {
        for shown in ["${LAN_KEY}", "${file:/run/key}", "${A}${B}"] {
            assert!(only_references(shown), "{shown}");
        }
        for hidden in [
            "",
            "written-key",
            "prefix-${LAN_KEY}",
            "${LAN_KEY}-suffix",
            "$${LAN_KEY}",
            "${LAN_KEY",
        ] {
            assert!(!only_references(hidden), "{hidden}");
        }
    }
}
