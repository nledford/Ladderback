//! A secret from a command: started without a shell, in a process group of
//! its own, under a time limit and a size limit.

use std::time::Duration;

use domain::secrets::{CommandLine, SecretError, SecretValue};

/// Runs `command` and gives the first line of its output.
pub(crate) async fn run(
    _command: &CommandLine,
    _limit: Duration,
) -> Result<SecretValue, SecretError> {
    todo!("run the command and give the first line of its output")
}
