//! Finds the message IDs product code names, for the unused-ID lint. It lexes
//! Rust with `proc-macro2`, so comments and strings never count, and it skips
//! test code.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

/// What one source file names through `msg::`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// The constants named: `CLI_LISTENING` for `msg::CLI_LISTENING`.
    pub names: BTreeSet<String>,
    /// Whether a `msg::*` import hides what is used.
    pub globs: bool,
}

/// The constants of `l10n::msg` that `source` names outside test code. An
/// error if the text isn't Rust a lexer can read.
pub fn scan(source: &str) -> Result<Usage, String> {
    let stream = TokenStream::from_str(source).map_err(|error| error.to_string())?;
    let mut usage = Usage::default();
    walk(stream, &mut usage);
    Ok(usage)
}

fn walk(stream: TokenStream, usage: &mut Usage) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    if has_inner_test_cfg(&tokens) {
        return;
    }
    let mut at = 0;
    while at < tokens.len() {
        if marks_test_code(&tokens, at) {
            at = skip_item(&tokens, at + 2);
            continue;
        }
        match &tokens[at] {
            TokenTree::Ident(ident) if ident == "msg" && is_path_separator(&tokens, at + 1) => {
                match tokens.get(at + 3) {
                    Some(TokenTree::Ident(name)) => {
                        usage.names.insert(name.to_string());
                    }
                    Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                        collect_idents(group.stream(), &mut usage.names);
                    }
                    Some(TokenTree::Punct(star)) if star.as_char() == '*' => usage.globs = true,
                    _ => {}
                }
            }
            TokenTree::Group(group) => walk(group.stream(), usage),
            _ => {}
        }
        at += 1;
    }
}

fn is_path_separator(tokens: &[TokenTree], at: usize) -> bool {
    matches!(
        (tokens.get(at), tokens.get(at + 1)),
        (Some(TokenTree::Punct(first)), Some(TokenTree::Punct(second)))
            if first.as_char() == ':' && second.as_char() == ':'
    )
}

/// Whether the tokens at `at` are an outer attribute (`#[…]`) that marks the
/// item after it as test code.
fn marks_test_code(tokens: &[TokenTree], at: usize) -> bool {
    let (Some(TokenTree::Punct(hash)), Some(TokenTree::Group(attribute))) =
        (tokens.get(at), tokens.get(at + 1))
    else {
        return false;
    };
    hash.as_char() == '#'
        && attribute.delimiter() == Delimiter::Bracket
        && is_test_attribute(&attribute.stream().into_iter().collect::<Vec<_>>())
}

/// Whether `tokens` hold an inner attribute `#![cfg(…)]` whose condition only
/// holds under test, which makes the whole enclosing file or block test code.
fn has_inner_test_cfg(tokens: &[TokenTree]) -> bool {
    tokens.windows(3).any(|window| match window {
        [
            TokenTree::Punct(hash),
            TokenTree::Punct(bang),
            TokenTree::Group(attribute),
        ] => {
            hash.as_char() == '#'
                && bang.as_char() == '!'
                && attribute.delimiter() == Delimiter::Bracket
                && is_test_cfg(&attribute.stream().into_iter().collect::<Vec<_>>())
        }
        _ => false,
    })
}

/// Whether an attribute's tokens (without the `#[` and `]`) mark test code:
/// a `cfg` that only holds under test, or a path ending in `test` such as
/// `test`, `tokio::test` or `tokio::test(flavor = "multi_thread")`.
fn is_test_attribute(attribute: &[TokenTree]) -> bool {
    is_test_cfg(attribute) || ends_in_test(attribute)
}

/// `cfg(PREDICATE)` where PREDICATE only holds under test.
fn is_test_cfg(attribute: &[TokenTree]) -> bool {
    match attribute {
        [TokenTree::Ident(name), TokenTree::Group(predicate)]
            if name == "cfg" && predicate.delimiter() == Delimiter::Parenthesis =>
        {
            only_under_test(&predicate.stream().into_iter().collect::<Vec<_>>())
        }
        _ => false,
    }
}

/// A path of identifiers joined by `::` that ends in `test`, with an optional
/// argument group after it.
fn ends_in_test(attribute: &[TokenTree]) -> bool {
    let path = match attribute {
        [path @ .., TokenTree::Group(_)] => path,
        path => path,
    };
    let mut chunks = path.chunks(3).peekable();
    while let Some(chunk) = chunks.next() {
        let joined = chunks.peek().is_some();
        let well_formed = match chunk {
            [
                TokenTree::Ident(_),
                TokenTree::Punct(first),
                TokenTree::Punct(second),
            ] => joined && first.as_char() == ':' && second.as_char() == ':',
            [TokenTree::Ident(segment)] => !joined && segment == "test",
            _ => false,
        };
        if !well_formed {
            return false;
        }
    }
    !path.is_empty()
}

/// Whether a `cfg` predicate can only be true under test: `test`, an `all(…)`
/// with such an argument, or an `any(…)` whose every argument is such.
fn only_under_test(predicate: &[TokenTree]) -> bool {
    match predicate {
        [TokenTree::Ident(name)] => name == "test",
        [TokenTree::Ident(name), TokenTree::Group(arguments)]
            if arguments.delimiter() == Delimiter::Parenthesis =>
        {
            let arguments = split_arguments(arguments.stream());
            if name == "all" {
                arguments.iter().any(|argument| only_under_test(argument))
            } else if name == "any" {
                !arguments.is_empty() && arguments.iter().all(|argument| only_under_test(argument))
            } else {
                false
            }
        }
        _ => false,
    }
}

/// The comma-separated arguments of a group; a trailing comma adds none.
fn split_arguments(stream: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut arguments = vec![Vec::new()];
    for token in stream {
        match &token {
            TokenTree::Punct(punct) if punct.as_char() == ',' => arguments.push(Vec::new()),
            _ => arguments
                .last_mut()
                .into_iter()
                .for_each(|last| last.push(token.clone())),
        }
    }
    arguments.retain(|argument| !argument.is_empty());
    arguments
}

/// The index after the item that starts at `from`: it ends at its braced body
/// or at a `;`.
fn skip_item(tokens: &[TokenTree], from: usize) -> usize {
    let mut at = from;
    while let Some(token) = tokens.get(at) {
        at += 1;
        match token {
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => break,
            TokenTree::Punct(punct) if punct.as_char() == ';' => break,
            _ => {}
        }
    }
    at
}

fn collect_idents(stream: TokenStream, names: &mut BTreeSet<String>) {
    for token in stream {
        match token {
            TokenTree::Ident(ident) => {
                names.insert(ident.to_string());
            }
            TokenTree::Group(group) => collect_idents(group.stream(), names),
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// `files` without the ones only test code declares: the files `#[cfg(test)]
/// mod name;` points at, and the files those declare in turn. `read` gives a
/// file's text, or `None` if there is no such file. An error if a test module
/// has no file, or one can't be read as Rust.
#[allow(dead_code, reason = "the fix makes `product_sources` use it")]
pub fn exclude_test_modules(
    _files: Vec<PathBuf>,
    _read: impl Fn(&Path) -> Option<String>,
) -> Result<Vec<PathBuf>, String> {
    todo!("exclude_test_modules")
}

/// The workspace's product sources: `src/` and every crate's `src/`, minus
/// tests, benches, examples and build output.
pub fn product_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for top in ["src", "crates"] {
        collect(&root.join(top), &mut found);
    }
    found.sort();
    found
}

fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            let skipped = matches!(
                name.as_ref(),
                "target" | "tests" | "benches" | "examples" | "node_modules" | "dist" | ".git"
            );
            if !skipped {
                collect(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") && name != "tests.rs"
        {
            found.push(path);
        }
    }
}
