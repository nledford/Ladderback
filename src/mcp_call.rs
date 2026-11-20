//! `ladderback mcp call` (§4.6): runs MCP tools once, without a client,
//! through the dispatch `ladderback mcp` serves, and prints one JSON line on
//! standard output for each call: its tool's result, or its refusal.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read as _, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, MessageId, msg};

use crate::{McpMode, Session, output, say, text};

/// The longest line of a script read, in bytes: the longest message the
/// server reads from a client.
const MAX_LINE: usize = ::mcp::MAX_MESSAGE_BYTES;

// The command line of `ladderback mcp call`. Its description, and each of its
// arguments', is the catalog's (`cli-help-mcp-call-*`), set when help is said.
#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("calls").required(true).args(["tool", "script"])))]
#[command(group(clap::ArgGroup::new("call-mode").args(["connect", "headless", "scratch"])))]
pub(crate) struct CallArgs {
    tool: Option<String>,
    #[arg(value_name = "NAME=VALUE", requires = "tool")]
    words: Vec<String>,
    #[arg(
        long = "args",
        value_name = "JSON",
        requires = "tool",
        conflicts_with = "words"
    )]
    arguments: Option<String>,
    #[arg(long, value_name = "FILE")]
    script: Option<PathBuf>,
    #[arg(long, requires = "script")]
    keep_going: bool,
    #[arg(long, value_name = "URL")]
    connect: Option<String>,
    #[arg(long, value_name = "PATH", conflicts_with_all = ["headless", "scratch"])]
    token_file: Option<PathBuf>,
    #[arg(long)]
    headless: bool,
    #[arg(long)]
    scratch: bool,
}

/// What a one-shot command calls: one tool, or the calls of a script.
enum Calls {
    One(::mcp::Call),
    Script {
        input: Box<dyn BufRead>,
        /// The script's path as it was given: `-` for standard input.
        file: PathBuf,
        keep_going: bool,
    },
}

/// Runs `ladderback mcp call`. Its session resolves secrets or reads a
/// token as the server's does, so it starts protected the same way, before
/// it reads the script or its calls, and keeps its log as the server does.
pub(crate) fn call(args: CallArgs, plain: bool) -> ExitCode {
    let recent = crate::recent_log();
    let localizer = crate::start_protected(plain, Some(Arc::clone(&recent)));
    crate::finish(&localizer, run(&localizer, args, recent))
}

/// Reads the calls, starts the session the mode names, makes the calls,
/// and ends the session: 0 when every call worked, 1 when one failed or the
/// session couldn't start, 2 when the command line can't be read.
fn run(
    localizer: &Localizer,
    args: CallArgs,
    recent: Arc<dyn app::ports::RecentLogStore>,
) -> io::Result<ExitCode> {
    let mode = if args.headless {
        McpMode::Headless
    } else if args.scratch {
        McpMode::Scratch
    } else {
        McpMode::Connect(
            args.connect
                .unwrap_or_else(|| ::mcp::DEFAULT_DAEMON_URL.to_owned()),
            args.token_file,
        )
    };
    let calls = match (args.tool, args.script) {
        (Some(tool), _) => {
            let read = match args.arguments {
                Some(arguments) => ::mcp::Call::of_object(&tool, &arguments),
                None => ::mcp::Call::of_words(&tool, &args.words),
            };
            match read {
                // The session ends after the call, and its end stops the
                // backup a run starts: refused rather than reported as a
                // success that never finishes.
                Ok(call) if call.tool() == "run_plan" && !matches!(mode, McpMode::Connect(..)) => {
                    say(localizer, msg::CLI_MCP_CALL_RUN_ALONE, &[])?;
                    return Ok(ExitCode::from(2));
                }
                // Only connect mode offers the proposal tools (§4.6).
                Ok(call)
                    if !matches!(mode, McpMode::Connect(..))
                        && !call.offered_in(::mcp::Offer::InProcess) =>
                {
                    let (id, message_args) =
                        refusal_of_the_command_line(::mcp::CallRefused::NoTool, &tool);
                    say(localizer, id, &message_args)?;
                    return Ok(ExitCode::from(2));
                }
                Ok(call) => Calls::One(call),
                Err(refused) => {
                    let (id, message_args) = refusal_of_the_command_line(refused, &tool);
                    say(localizer, id, &message_args)?;
                    return Ok(ExitCode::from(2));
                }
            }
        }
        (None, Some(file)) => match script(&file) {
            Ok(input) => Calls::Script {
                input,
                file,
                keep_going: args.keep_going,
            },
            Err(error) => {
                say(
                    localizer,
                    msg::CLI_MCP_CALL_SCRIPT_UNREADABLE,
                    &[("file", text(file.display())), ("detail", text(error))],
                )?;
                return Ok(ExitCode::FAILURE);
            }
        },
        // The parser lets neither through without one or the other.
        (None, None) => return Ok(ExitCode::from(2)),
    };
    let session = match Session::start(localizer, &mode, recent)? {
        Ok(session) => session,
        Err(status) => return Ok(status),
    };
    let made = make(localizer, &session, calls);
    session.end();
    Ok(if made? {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// The message for a call on the command line that can't be read, and its
/// arguments.
fn refusal_of_the_command_line(
    refused: ::mcp::CallRefused,
    tool: &str,
) -> (MessageId, Vec<(&'static str, MsgArg)>) {
    match refused {
        ::mcp::CallRefused::NoTool => (msg::CLI_MCP_CALL_NO_TOOL, vec![("tool", text(tool))]),
        ::mcp::CallRefused::NotAPair(position) => (
            msg::CLI_MCP_CALL_NOT_A_PAIR,
            vec![("position", MsgArg::Count(count(position)))],
        ),
        ::mcp::CallRefused::Repeated(name) => {
            (msg::CLI_MCP_CALL_REPEATED, vec![("name", text(name))])
        }
        // A command line's call is never a script's line.
        ::mcp::CallRefused::NotAnObject | ::mcp::CallRefused::NotACall => {
            (msg::CLI_MCP_CALL_NOT_AN_OBJECT, Vec::new())
        }
    }
}

/// `n` as a count.
fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The script at `file`, or standard input for `-`.
fn script(file: &std::path::Path) -> io::Result<Box<dyn BufRead>> {
    if file.as_os_str() == "-" {
        return Ok(Box::new(io::stdin().lock()));
    }
    Ok(Box::new(BufReader::new(File::open(file)?)))
}

/// Makes `calls` in `session`, printing a line for each: whether every
/// call worked. A script stops at the first that fails unless it is to keep
/// going.
fn make(localizer: &Localizer, session: &Session, calls: Calls) -> io::Result<bool> {
    let mut out = io::stdout().lock();
    match calls {
        Calls::One(call) => made(session, call, &mut out),
        Calls::Script {
            mut input,
            file,
            keep_going,
        } => {
            let mut all = true;
            let mut line = Vec::new();
            let mut number: u64 = 0;
            loop {
                number = number.saturating_add(1);
                let worked = match next_line(&mut input, &mut line) {
                    Ok(Some(Line::Read)) if line.iter().all(u8::is_ascii_whitespace) => continue,
                    Ok(Some(Line::Read)) => {
                        match std::str::from_utf8(&line)
                            .map_err(|_| ::mcp::CallRefused::NotACall)
                            .and_then(|line| {
                                // Only connect mode offers the proposal
                                // tools: elsewhere a line calling one names
                                // no tool, as on the command line (§4.6).
                                let call = ::mcp::Call::of_line(line)?;
                                if call.offered_in(session.server.offer()) {
                                    Ok(call)
                                } else {
                                    Err(::mcp::CallRefused::NoTool)
                                }
                            }) {
                            Ok(call) => made(session, call, &mut out)?,
                            Err(refused) => {
                                say_line(localizer, number, &refused)?;
                                false
                            }
                        }
                    }
                    Ok(Some(Line::TooLong)) => {
                        say(
                            localizer,
                            msg::CLI_MCP_CALL_LINE_TOO_LONG,
                            &[
                                ("line", MsgArg::Count(number)),
                                ("limit", MsgArg::Bytes(count(MAX_LINE))),
                            ],
                        )?;
                        false
                    }
                    Ok(None) => break,
                    Err(error) => {
                        say(
                            localizer,
                            msg::CLI_MCP_CALL_SCRIPT_UNREADABLE,
                            &[("file", text(file.display())), ("detail", text(error))],
                        )?;
                        return Ok(false);
                    }
                };
                all &= worked;
                if !worked && !keep_going {
                    break;
                }
            }
            Ok(all)
        }
    }
}

/// Makes `call` in `session` and prints its line: whether it worked. A
/// refusal's sentence, in the catalog's words, is said on stderr too, after
/// the line ([`output::write_json`] flushes it), so the agent or person
/// reading learns what to do next; the line stays as it is, for scripts
/// (§4.6).
fn made(session: &Session, call: ::mcp::Call, out: &mut impl Write) -> io::Result<bool> {
    let tool = call.tool();
    let answer = session.call(call);
    output::write_json(out, &answer.outcome(tool))?;
    if let ::mcp::Answer::Refused(error) = &answer {
        let sentence = session.server.sentence(&output::escaped_error(error));
        output::to_stderr(&format!("ladderback: {sentence}\n"))?;
    }
    Ok(answer.is_ok())
}

/// Says why the script's line `number` isn't a call. Nothing of the line is
/// repeated, not even the tool it names: it may hold a profile's password.
fn say_line(localizer: &Localizer, number: u64, refused: &::mcp::CallRefused) -> io::Result<()> {
    let id = match refused {
        ::mcp::CallRefused::NoTool => msg::CLI_MCP_CALL_LINE_NO_TOOL,
        _ => msg::CLI_MCP_CALL_LINE_NOT_A_CALL,
    };
    say(localizer, id, &[("line", MsgArg::Count(number))])
}

/// What [`next_line`] read.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    /// A line, in the buffer, without its end.
    Read,
    /// A line longer than [`MAX_LINE`], read to its end and dropped.
    TooLong,
}

/// Reads the next line of `input` into `line`; `None` at the end of the
/// input. Never more than [`MAX_LINE`] and its end are held.
fn next_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Option<Line>> {
    line.clear();
    let limit = u64::try_from(MAX_LINE)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    if input.by_ref().take(limit).read_until(b'\n', line)? == 0 {
        return Ok(None);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        return Ok(Some(Line::Read));
    }
    if line.len() <= MAX_LINE {
        // The input's last line, with no end.
        return Ok(Some(Line::Read));
    }
    line.clear();
    loop {
        let buffered = input.fill_buf()?;
        if buffered.is_empty() {
            break;
        }
        match buffered.iter().position(|&byte| byte == b'\n') {
            Some(end) => {
                input.consume(end.saturating_add(1));
                break;
            }
            None => {
                let length = buffered.len();
                input.consume(length);
            }
        }
    }
    Ok(Some(Line::TooLong))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(input: &[u8]) -> Vec<Option<Line>> {
        let mut input = input;
        let mut line = Vec::new();
        let mut read = Vec::new();
        loop {
            let next = next_line(&mut input, &mut line).unwrap();
            let done = next.is_none();
            read.push(next);
            if done {
                return read;
            }
        }
    }

    #[test]
    fn a_line_is_read_without_its_end_and_the_last_needs_none() {
        let mut input: &[u8] = b"one\ntwo";
        let mut line = Vec::new();
        assert_eq!(next_line(&mut input, &mut line).unwrap(), Some(Line::Read));
        assert_eq!(line, b"one");
        assert_eq!(next_line(&mut input, &mut line).unwrap(), Some(Line::Read));
        assert_eq!(line, b"two");
        assert_eq!(next_line(&mut input, &mut line).unwrap(), None);
    }

    #[test]
    fn a_line_over_the_limit_is_dropped_to_its_end_and_the_next_is_read() {
        let mut input = vec![b'x'; MAX_LINE];
        input.push(b'\n');
        input.extend(vec![b'y'; MAX_LINE + 1]);
        input.extend(b"\nlast\n");
        assert_eq!(
            lines(&input),
            [
                Some(Line::Read),
                Some(Line::TooLong),
                Some(Line::Read),
                None
            ]
        );
        let mut tail: &[u8] = &input[MAX_LINE + 1..];
        let mut line = Vec::new();
        next_line(&mut tail, &mut line).unwrap();
        next_line(&mut tail, &mut line).unwrap();
        assert_eq!(line, b"last");
    }

    #[test]
    fn a_last_line_over_the_limit_with_no_end_is_dropped() {
        assert_eq!(
            lines(&vec![b'z'; MAX_LINE + 1]),
            [Some(Line::TooLong), None]
        );
    }
}
