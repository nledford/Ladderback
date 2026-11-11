//! Commands that run around an operation (technical design §6.1, decision
//! 10): rustic's four hook lists at three levels, the agent's, a
//! repository's and a plan's. The order they run in belongs to the runner.

use crate::secrets::CommandLine;

use super::{InvalidValue, ValueKind};

/// What a failing hook does to the operation it runs for, as rustic's
/// `on-failure` says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnFailure {
    /// The operation fails (the default).
    #[default]
    Error,
    /// The operation goes on, and the failure is a warning.
    Warn,
    /// The operation goes on, and the failure is not reported.
    Ignore,
}

impl OnFailure {
    /// Reads `error`, `warn` or `ignore`, in lower case.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        match text {
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "ignore" => Ok(Self::Ignore),
            _ => Err(InvalidValue::new(ValueKind::OnFailure, text)),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Ignore => "ignore",
        }
    }
}

/// What a refused command reports: its kind and none of its text, since a
/// command line may hold a secret (`curl -u demo:hunter2`).
fn no_command() -> InvalidValue {
    InvalidValue::new(ValueKind::HookCommand, "")
}

/// One command to run, and what its failure means.
///
/// An error never repeats the command: a command line may hold a secret
/// (`curl -u demo:hunter2`), and so `Debug` shows only the program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookEntry {
    pub command: CommandLine,
    pub on_failure: OnFailure,
}

impl HookEntry {
    /// An entry written as one string: split into words the way a POSIX
    /// shell splits them, as rustic does, and run without a shell. A failure
    /// is an error, as for rustic.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let command = CommandLine::parse(text).map_err(|_| no_command())?;
        Ok(Self {
            command,
            on_failure: OnFailure::default(),
        })
    }

    /// An entry written as a table: the program and each argument are taken
    /// as they are, with nothing split, so a path with a space in it is one
    /// word.
    pub fn new(
        program: &str,
        args: impl IntoIterator<Item = impl Into<String>>,
        on_failure: OnFailure,
    ) -> Result<Self, InvalidValue> {
        let command = CommandLine::new(program, args).map_err(|_| no_command())?;
        Ok(Self {
            command,
            on_failure,
        })
    }
}

/// The hooks of one level, in the order they were written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hooks {
    pub run_before: Vec<HookEntry>,
    pub run_after: Vec<HookEntry>,
    pub run_failed: Vec<HookEntry>,
    pub run_finally: Vec<HookEntry>,
}

impl Hooks {
    pub fn is_empty(&self) -> bool {
        self.run_before.is_empty()
            && self.run_after.is_empty()
            && self.run_failed.is_empty()
            && self.run_finally.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a refused command reports: its kind, and none of its text.
    fn no_command() -> InvalidValue {
        InvalidValue::new(ValueKind::HookCommand, "")
    }

    fn entry(text: &str) -> HookEntry {
        HookEntry::parse(text).unwrap()
    }

    #[test]
    fn a_hook_that_says_nothing_else_fails_the_operation_when_it_fails() {
        assert_eq!(OnFailure::default(), OnFailure::Error);
        assert_eq!(entry("true").on_failure, OnFailure::Error);
    }

    #[test]
    fn an_on_failure_is_error_warn_or_ignore() {
        for (text, on_failure) in [
            ("error", OnFailure::Error),
            ("warn", OnFailure::Warn),
            ("ignore", OnFailure::Ignore),
        ] {
            assert_eq!(OnFailure::parse(text), Ok(on_failure));
            assert_eq!(on_failure.as_str(), text);
        }
        for text in ["", "Error", "WARN", "fail", "warn ", "skip"] {
            assert_eq!(
                OnFailure::parse(text),
                Err(InvalidValue::new(ValueKind::OnFailure, text)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn an_entry_written_as_a_string_is_split_like_a_shell_splits_words() {
        let hook = entry("notify-send 'backup done' --urgency=low");
        assert_eq!(hook.command.program(), "notify-send");
        assert_eq!(hook.command.args(), ["backup done", "--urgency=low"]);
        assert_eq!(hook.on_failure, OnFailure::Error);
    }

    #[test]
    fn an_entry_written_as_a_table_is_not_split() {
        let hook = HookEntry::new(
            "/opt/demo tools/run it",
            ["a b", "--flag", "'quoted'"],
            OnFailure::Warn,
        )
        .unwrap();
        assert_eq!(hook.command.program(), "/opt/demo tools/run it");
        assert_eq!(hook.command.args(), ["a b", "--flag", "'quoted'"]);
        assert_eq!(hook.on_failure, OnFailure::Warn);
        let bare = HookEntry::new("true", Vec::<String>::new(), OnFailure::Ignore).unwrap();
        assert!(bare.command.args().is_empty());
        assert_eq!(bare.on_failure, OnFailure::Ignore);
    }

    #[test]
    fn a_command_that_names_no_program_is_refused_without_repeating_it() {
        for text in ["", "   ", "# only a comment", "echo 'never closed", "\"\""] {
            assert_eq!(HookEntry::parse(text), Err(no_command()), "{text:?}");
        }
        assert_eq!(
            HookEntry::new("", ["--token", "hunter2"], OnFailure::Warn),
            Err(no_command())
        );
        let leaky = HookEntry::parse("notify --token hunter2 'never closed").unwrap_err();
        assert!(!format!("{leaky:?}{leaky}").contains("hunter2"));
    }

    // A hook runs in the daemon's working directory, so a program named by a
    // relative path would be a different one, or none, by how it was started.
    #[test]
    fn only_a_program_named_by_a_relative_path_is_refused_with_the_program_alone() {
        for (text, program) in [
            ("./notify.sh --token hunter2", "./notify.sh"),
            ("bin/notify --token hunter2", "bin/notify"),
            ("../notify", "../notify"),
            ("~/bin/notify", "~/bin/notify"),
            ("'demo tools/notify' done", "demo tools/notify"),
        ] {
            let refused = Err(InvalidValue::new(ValueKind::HookProgram, program));
            assert_eq!(HookEntry::parse(text), refused, "{text:?}");
            assert_eq!(
                HookEntry::new(program, ["--token", "hunter2"], OnFailure::Warn),
                refused,
                "{program:?}"
            );
        }
        // A name is looked up on `PATH`, and an absolute path is the same
        // file wherever the daemon started.
        for program in ["notify-send", "~notify", "..", "/usr/local/bin/notify"] {
            assert_eq!(entry(program).command.program(), program);
            let hook = HookEntry::new(program, ["done"], OnFailure::Ignore).unwrap();
            assert_eq!(hook.command.program(), program);
        }
    }

    #[test]
    fn an_entry_never_shows_its_arguments() {
        let hook = entry("login --password hunter2");
        let hooks = Hooks {
            run_before: vec![hook.clone()],
            ..Hooks::default()
        };
        for shown in [format!("{hook:?}"), format!("{hooks:?}")] {
            assert!(shown.contains("login"), "{shown}");
            assert!(!shown.contains("hunter2"), "{shown}");
        }
    }

    #[test]
    fn hooks_that_name_no_command_are_empty() {
        assert!(Hooks::default().is_empty());
    }

    #[test]
    fn hooks_with_a_command_in_any_list_are_not_empty() {
        let one = vec![entry("true")];
        type Setter = fn(&mut Hooks, Vec<HookEntry>);
        let lists: [(&str, Setter); 4] = [
            ("run-before", |h, e| h.run_before = e),
            ("run-after", |h, e| h.run_after = e),
            ("run-failed", |h, e| h.run_failed = e),
            ("run-finally", |h, e| h.run_finally = e),
        ];
        for (name, set) in lists {
            let mut hooks = Hooks::default();
            set(&mut hooks, one.clone());
            assert!(!hooks.is_empty(), "{name} was ignored");
        }
    }
}
