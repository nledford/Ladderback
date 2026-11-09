//! Ladderback's own command-line messages (technical design §8.4): the
//! banner other tools read, and the rule that no message changes the values
//! it is given.

use api::MsgArg;
use l10n::{MessageId, msg};

use crate::support::{borrowed, english, source_locale};

#[test]
fn the_listening_banner_keeps_the_text_other_tools_parse() {
    // `ui-tests` and the end-to-end steps read the address after "listening on".
    let shown = english().message(
        MessageId::new("cli-listening"),
        &[("url", MsgArg::Text("http://127.0.0.1:9898".to_owned()))],
    );
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    assert_eq!(shown.text(), "listening on http://127.0.0.1:9898");
}

#[test]
fn a_cli_message_shows_each_argument_exactly_as_given() {
    let source = source_locale();
    let localizer = english();
    let cli: Vec<MessageId> = msg::ALL
        .iter()
        .copied()
        .filter(|id| id.as_str().starts_with("cli-"))
        .collect();
    assert!(!cli.is_empty());
    for id in cli {
        let value = source
            .messages
            .get(id.as_str())
            .and_then(|message| message.value.as_ref())
            .unwrap_or_else(|| panic!("`{}` has a value", id.as_str()));
        // Text no formatter may change: digits a locale would group, braces,
        // quotes, a path.
        let args: Vec<(String, MsgArg)> = value
            .variables
            .iter()
            .map(|name| {
                let text = format!("{name}: /home/demo/it's \"1,000\" {{ 9898 }}");
                (name.clone(), MsgArg::Text(text))
            })
            .collect();
        let shown = localizer.message(id, &borrowed(&args));
        assert!(
            shown.problems().is_empty(),
            "{}: {:?}",
            id.as_str(),
            shown.problems()
        );
        for (name, arg) in &args {
            let MsgArg::Text(text) = arg else {
                panic!("a text argument");
            };
            assert!(
                shown.text().contains(text.as_str()),
                "{} changes `{name}`: {shown}",
                id.as_str()
            );
        }
    }
}
