//! A command line as rustic reads one (`password-command`, hooks): a program
//! and its arguments, split the way a POSIX shell splits words, and run
//! without a shell.

use super::REDACTED;

/// A program and its arguments. Nothing runs it here; the adapter that does
/// starts the program directly, so no shell ever sees the arguments.
///
/// `Debug` shows the program and hides every argument: `echo hunter2` is a
/// password command somebody will write, and a log must not show it.
#[derive(Clone, PartialEq, Eq)]
pub struct CommandLine {
    program: String,
    args: Vec<String>,
}

impl std::fmt::Debug for CommandLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let args: Vec<&str> = self.args.iter().map(|_| REDACTED).collect();
        f.debug_struct("CommandLine")
            .field("program", &self.program)
            .field("args", &args)
            .finish()
    }
}

/// Why a command line couldn't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandLineError {
    /// No program: the text is empty, only white space or a comment, or the
    /// program's name is empty.
    Empty,
    /// A single or double quote is never closed.
    UnclosedQuote,
}

impl std::fmt::Display for CommandLineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for CommandLineError {}

impl CommandLine {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, CommandLineError> {
        let program = program.into();
        if program.is_empty() {
            return Err(CommandLineError::Empty);
        }
        Ok(Self {
            program,
            args: args.into_iter().map(Into::into).collect(),
        })
    }

    /// Splits `text` as rustic does (the `shell_words` crate): white space
    /// separates words, quotes and backslashes group and escape, `#` starts a
    /// comment. No variable, command or glob is ever expanded.
    pub fn parse(text: &str) -> Result<Self, CommandLineError> {
        let mut words = split(text)?.into_iter();
        let program = words.next().ok_or(CommandLineError::Empty)?;
        Self::new(program, words)
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }
}

#[derive(Clone, Copy)]
enum State {
    /// Between words.
    Delimiter,
    /// After a backslash that starts a word.
    Backslash,
    /// In an unquoted word.
    Unquoted,
    /// After a backslash in an unquoted word.
    UnquotedBackslash,
    SingleQuoted,
    DoubleQuoted,
    /// After a backslash inside double quotes.
    DoubleQuotedBackslash,
    Comment,
}

/// The words of `text`, by the rules of POSIX shell quoting without any
/// expansion. The same rules, state for state, as `shell_words::split`: the
/// tests compare the two on every short string over the characters that
/// matter.
fn split(text: &str) -> Result<Vec<String>, CommandLineError> {
    use State::{
        Backslash, Comment, Delimiter, DoubleQuoted, DoubleQuotedBackslash, SingleQuoted, Unquoted,
        UnquotedBackslash,
    };
    let mut words = Vec::new();
    let mut word = String::new();
    let mut state = Delimiter;
    for c in text.chars() {
        state = match (state, c) {
            (Delimiter, '\'') => SingleQuoted,
            (Delimiter, '"') => DoubleQuoted,
            (Delimiter, '\\') => Backslash,
            (Delimiter, ' ' | '\t' | '\n') => Delimiter,
            (Delimiter, '#') => Comment,
            (Delimiter, c) => {
                word.push(c);
                Unquoted
            }
            // A backslash and a newline join two lines.
            (Backslash, '\n') => Delimiter,
            (UnquotedBackslash, '\n') => Unquoted,
            (Backslash | UnquotedBackslash, c) => {
                word.push(c);
                Unquoted
            }
            (Unquoted, '\'') => SingleQuoted,
            (Unquoted, '"') => DoubleQuoted,
            (Unquoted, '\\') => UnquotedBackslash,
            (Unquoted, ' ' | '\t' | '\n') => {
                words.push(std::mem::take(&mut word));
                Delimiter
            }
            (Unquoted, c) => {
                word.push(c);
                Unquoted
            }
            (SingleQuoted, '\'') => Unquoted,
            (SingleQuoted, c) => {
                word.push(c);
                SingleQuoted
            }
            (DoubleQuoted, '"') => Unquoted,
            (DoubleQuoted, '\\') => DoubleQuotedBackslash,
            (DoubleQuoted, c) => {
                word.push(c);
                DoubleQuoted
            }
            (DoubleQuotedBackslash, '\n') => DoubleQuoted,
            (DoubleQuotedBackslash, c @ ('$' | '`' | '"' | '\\')) => {
                word.push(c);
                DoubleQuoted
            }
            (DoubleQuotedBackslash, c) => {
                word.push('\\');
                word.push(c);
                DoubleQuoted
            }
            (Comment, '\n') => Delimiter,
            (Comment, _) => Comment,
        };
    }
    match state {
        Delimiter | Comment => {}
        // A backslash that ends the text is a backslash.
        Backslash | UnquotedBackslash => {
            word.push('\\');
            words.push(word);
        }
        Unquoted => words.push(word),
        SingleQuoted | DoubleQuoted | DoubleQuotedBackslash => {
            return Err(CommandLineError::UnclosedQuote);
        }
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        split(text).unwrap()
    }

    #[test]
    fn a_command_line_is_a_program_and_its_arguments() {
        let command =
            CommandLine::parse("security find-generic-password -s rustic-nas -w").unwrap();
        assert_eq!(command.program(), "security");
        assert_eq!(
            command.args(),
            ["find-generic-password", "-s", "rustic-nas", "-w"]
        );
    }

    #[test]
    fn a_program_alone_has_no_arguments() {
        let command = CommandLine::parse("  pass-show-nas\n").unwrap();
        assert_eq!(command.program(), "pass-show-nas");
        assert!(command.args().is_empty());
    }

    #[test]
    fn quotes_keep_words_together_and_escapes_keep_their_character() {
        assert_eq!(words(r#"a 'b c' "d e" f\ g"#), ["a", "b c", "d e", "f g"]);
        // Quoted and unquoted pieces of one word join.
        assert_eq!(words(r#"a'b c'd"e f"g"#), ["ab cde fg"]);
        // An empty pair of quotes is an empty argument.
        assert_eq!(words("a '' \"\" b"), ["a", "", "", "b"]);
    }

    #[test]
    fn inside_double_quotes_only_four_characters_and_a_newline_are_escaped() {
        assert_eq!(words(r#""a\"b\\c\$d\`e""#), [r#"a"b\c$d`e"#]);
        assert_eq!(words(r#""a\qb""#), [r"a\qb"]);
        assert_eq!(words("\"a\\\nb\""), ["ab"]);
    }

    #[test]
    fn single_quotes_keep_everything_including_backslashes() {
        assert_eq!(words(r"'a\b $HOME #x'"), [r"a\b $HOME #x"]);
    }

    #[test]
    fn a_backslash_before_a_newline_joins_the_lines() {
        assert_eq!(words("a\\\nb"), ["ab"]);
        assert_eq!(words("a \\\nb"), ["a", "b"]);
    }

    #[test]
    fn a_backslash_that_ends_the_text_is_a_backslash() {
        assert_eq!(words("a\\"), ["a\\"]);
        assert_eq!(words("a b \\"), ["a", "b", "\\"]);
    }

    #[test]
    fn a_hash_at_the_start_of_a_word_starts_a_comment_to_the_end_of_the_line() {
        assert_eq!(words("a #b c\nd"), ["a", "d"]);
        assert_eq!(words("a#b"), ["a#b"]);
        assert_eq!(words("# only a comment"), Vec::<String>::new());
    }

    #[test]
    fn tabs_and_newlines_separate_words_like_spaces() {
        assert_eq!(words("a\tb\nc  d"), ["a", "b", "c", "d"]);
    }

    #[test]
    fn nothing_is_expanded() {
        assert_eq!(
            words("echo $HOME ~ *.txt `id` $(id)"),
            ["echo", "$HOME", "~", "*.txt", "`id`", "$(id)"]
        );
    }

    #[test]
    fn a_quote_that_is_never_closed_is_an_error() {
        for text in ["'a", "a 'b", "\"a", "a \"b", "\"a\\"] {
            assert_eq!(
                CommandLine::parse(text),
                Err(CommandLineError::UnclosedQuote),
                "{text}"
            );
        }
    }

    #[test]
    fn a_command_line_without_a_program_is_an_error() {
        for text in ["", "   ", "\n", "# comment", "''", "\"\" a"] {
            assert_eq!(
                CommandLine::parse(text),
                Err(CommandLineError::Empty),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_command_line_made_from_parts_needs_a_program() {
        assert_eq!(CommandLine::new("", ["a"]), Err(CommandLineError::Empty));
        let command = CommandLine::new("sh", ["-c", "echo hi"]).unwrap();
        assert_eq!(command.program(), "sh");
        assert_eq!(command.args(), ["-c", "echo hi"]);
    }

    /// `echo hunter2` is a password command a user may well write.
    #[test]
    fn the_debug_form_of_a_command_line_hides_its_arguments() {
        let command = CommandLine::parse("echo hunter2 --key 'other secret'").unwrap();
        let debug = format!("{command:?}");
        assert_eq!(
            debug,
            r#"CommandLine { program: "echo", args: ["***", "***", "***"] }"#
        );
    }

    /// Every string up to six characters over the characters that change
    /// the state of the splitter: whatever `shell_words`, which rustic uses,
    /// makes of it, so does `split`.
    #[test]
    fn splitting_agrees_with_shell_words_on_every_short_string() {
        const ALPHABET: [char; 9] = ['a', ' ', '\t', '\n', '\'', '"', '\\', '#', '$'];
        let mut checked = 0_u32;
        let mut frontier = vec![String::new()];
        for _ in 0..=6 {
            let mut next = Vec::new();
            for text in &frontier {
                let theirs = shell_words::split(text).map_err(|_| ());
                let mine = split(text).map_err(|_| ());
                assert_eq!(mine, theirs, "{text:?}");
                checked += 1;
                if text.chars().count() < 6 {
                    next.extend(ALPHABET.iter().map(|c| format!("{text}{c}")));
                }
            }
            frontier = next;
        }
        assert!(checked > 500_000, "only {checked} strings were compared");
    }
}
