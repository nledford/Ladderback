//! Finds the message IDs product code names, for the unused-ID lint. It lexes
//! Rust with `proc-macro2`, so comments and strings never count, and it skips
//! test code: inline, in the files a `#[cfg(test)] mod name;` points at, and in
//! files named `tests.rs`.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
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
/// file's text, or `None` if there is no such file. An error if a file can't be
/// read as Rust, or a test module has no file: a hole in the scan would pass
/// for a message nothing but tests uses.
pub fn exclude_test_modules(
    files: Vec<PathBuf>,
    read: impl Fn(&Path) -> Option<String>,
) -> Result<Vec<PathBuf>, String> {
    let mut test_files = BTreeSet::new();
    // (file, whether it is a `mod.rs`-like file, whether it is test code)
    let mut pending: Vec<(PathBuf, bool, bool)> = files
        .iter()
        .map(|file| (file.clone(), is_mod_rs(file), false))
        .collect();
    while let Some((file, mod_rs, in_test)) = pending.pop() {
        let shown = file.display();
        let source = read(&file).ok_or_else(|| format!("{shown}: cannot be read"))?;
        let declared = test_modules(&file, &source, mod_rs, in_test)
            .map_err(|error| format!("{shown}: {error}"))?;
        for module in declared {
            let found = module
                .candidates
                .iter()
                .find(|(candidate, _)| read(candidate).is_some());
            let Some((path, mod_rs)) = found else {
                let looked: Vec<_> = module
                    .candidates
                    .iter()
                    .map(|(candidate, _)| candidate.display().to_string())
                    .collect();
                return Err(format!(
                    "{shown}: `mod {};` is test code, and none of {looked:?} exists",
                    module.name
                ));
            };
            if test_files.insert(path.clone()) {
                pending.push((path.clone(), *mod_rs, true));
            }
        }
    }
    Ok(files
        .into_iter()
        .filter(|file| !test_files.contains(file))
        .collect())
}

/// A `mod name;` that is test code, and the files it can be.
struct TestModule {
    name: String,
    /// Each file the declaration can mean, and whether it is a `mod.rs`-like
    /// file, whose own modules sit beside it.
    candidates: Vec<(PathBuf, bool)>,
}

/// Whether the modules of the file at `path` are found beside it, not in a
/// folder named for it: crate roots (`lib.rs`, `main.rs`, `build.rs`, anything
/// in `src/bin`) and `mod.rs` files.
fn is_mod_rs(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    let folder = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    matches!(name, Some("mod.rs" | "lib.rs" | "main.rs" | "build.rs")) || folder == Some("bin")
}

/// The `mod name;` declarations in a file that are test code: marked by a
/// `cfg` that only holds under test, inside such an inline module, or in a
/// file that is test code as a whole (`in_test`, or an inner `#![cfg(test)]`).
fn test_modules(
    file: &Path,
    source: &str,
    mod_rs: bool,
    in_test: bool,
) -> Result<Vec<TestModule>, String> {
    let stream = TokenStream::from_str(source).map_err(|error| error.to_string())?;
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let parent = file.parent().unwrap_or_else(|| Path::new(""));
    let dir = match file.file_stem() {
        Some(stem) if !mod_rs => parent.join(stem),
        _ => parent.to_path_buf(),
    };
    let mut found = Vec::new();
    declarations(
        &tokens,
        &Folders {
            dir,
            path_base: parent.to_path_buf(),
        },
        in_test,
        &mut found,
    )?;
    Ok(found)
}

/// Where a module's `mod name;` and `#[path = "…"]` declarations look.
struct Folders {
    /// For `mod name;`: `name.rs` or `name/mod.rs` in here.
    dir: PathBuf,
    /// For `#[path = "…"]`: relative to here.
    path_base: PathBuf,
}

fn declarations(
    tokens: &[TokenTree],
    folders: &Folders,
    in_test: bool,
    found: &mut Vec<TestModule>,
) -> Result<(), String> {
    let in_test = in_test || has_inner_test_cfg(tokens);
    let mut at = 0;
    while at < tokens.len() {
        if marks_inner_attribute(tokens, at) {
            at += 3;
            continue;
        }
        let mut test = false;
        let mut path = None;
        while marks_attribute(tokens, at) {
            if let Some(TokenTree::Group(attribute)) = tokens.get(at + 1) {
                let attribute: Vec<_> = attribute.stream().into_iter().collect();
                test |= is_test_attribute(&attribute);
                path = path.or(path_attribute(&attribute)?);
            }
            at += 2;
        }
        if matches!(tokens.get(at), Some(TokenTree::Ident(word)) if word == "pub") {
            at += 1;
            if matches!(tokens.get(at), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
            {
                at += 1;
            }
        }
        match (tokens.get(at), tokens.get(at + 1), tokens.get(at + 2)) {
            (Some(TokenTree::Ident(word)), Some(TokenTree::Ident(name)), Some(next))
                if word == "mod" =>
            {
                at += 3;
                match next {
                    TokenTree::Punct(semicolon) if semicolon.as_char() == ';' => {
                        if in_test || test {
                            found.push(TestModule {
                                name: name.to_string(),
                                candidates: candidates(folders, &name.to_string(), path.as_deref()),
                            });
                        }
                    }
                    TokenTree::Group(body) if body.delimiter() == Delimiter::Brace => {
                        let inside = folders
                            .dir
                            .join(path.as_deref().unwrap_or(&name.to_string()));
                        let inner: Vec<_> = body.stream().into_iter().collect();
                        let folders = Folders {
                            dir: inside.clone(),
                            path_base: inside,
                        };
                        declarations(&inner, &folders, in_test || test, found)?;
                    }
                    _ => {}
                }
            }
            _ => at = skip_item(tokens, at),
        }
    }
    Ok(())
}

/// The files `mod name;` can mean: the path attribute's, or `name.rs` or
/// `name/mod.rs` in the module's folder.
fn candidates(folders: &Folders, name: &str, path: Option<&str>) -> Vec<(PathBuf, bool)> {
    match path {
        Some(path) => vec![(clean(&folders.path_base.join(path)), true)],
        None => vec![
            (folders.dir.join(format!("{name}.rs")), false),
            (folders.dir.join(name).join("mod.rs"), true),
        ],
    }
}

/// `path` without `.` and `a/..` components, which the file system would
/// resolve and a comparison of paths doesn't.
fn clean(path: &Path) -> PathBuf {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if cleaned.file_name().is_some() => {
                cleaned.pop();
            }
            other => cleaned.push(other),
        }
    }
    cleaned
}

/// Whether the tokens at `at` are an inner attribute, `#![…]`.
fn marks_inner_attribute(tokens: &[TokenTree], at: usize) -> bool {
    matches!(
        (tokens.get(at), tokens.get(at + 1), tokens.get(at + 2)),
        (Some(TokenTree::Punct(hash)), Some(TokenTree::Punct(bang)), Some(TokenTree::Group(group)))
            if hash.as_char() == '#' && bang.as_char() == '!' && group.delimiter() == Delimiter::Bracket
    )
}

/// Whether the tokens at `at` are the start of an outer attribute, `#[…]`.
fn marks_attribute(tokens: &[TokenTree], at: usize) -> bool {
    matches!(
        (tokens.get(at), tokens.get(at + 1)),
        (Some(TokenTree::Punct(hash)), Some(TokenTree::Group(group)))
            if hash.as_char() == '#' && group.delimiter() == Delimiter::Bracket
    )
}

/// The file of `path = "…"`, if the attribute is one. An error for a string a
/// lexer can't take at face value, which the scan would mistake for no path.
fn path_attribute(attribute: &[TokenTree]) -> Result<Option<String>, String> {
    let [
        TokenTree::Ident(name),
        TokenTree::Punct(equals),
        TokenTree::Literal(literal),
    ] = attribute
    else {
        return Ok(None);
    };
    if name != "path" || equals.as_char() != '=' {
        return Ok(None);
    }
    let text = literal.to_string();
    match text
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
    {
        Some(path) if !path.contains('\\') => Ok(Some(path.to_owned())),
        _ => Err(format!("`#[path = {text}]` is not a plain string")),
    }
}

/// The workspace's product sources: `src/` and every crate's `src/`, minus
/// tests, benches, examples, build output and the files of test modules.
pub fn product_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for top in ["src", "crates"] {
        collect(&root.join(top), &mut found);
    }
    found.sort();
    exclude_test_modules(found, |path| std::fs::read_to_string(path).ok())
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
