//! A build without the `pseudo` feature (technical design §8.4), which is how a
//! release is built. `en-XA` and `ar-XB` are then languages like any other
//! that no catalog serves: a user, or a stray `LANGUAGE`, who asks for one gets
//! American English. These tests run with `--no-default-features`, which
//! `just test` doesn't pass: `just architecture` checks that the feature's
//! dependency is out of a release build, and this lane that the code
//! behaves without it.

use api::MsgArg;
use l10n::{Direction, Localizer, msg};

use crate::support::tag;

#[test]
fn a_build_without_pseudolocales_serves_their_tags_in_american_english() {
    for asked in ["en-XA", "ar-XB"] {
        let localizer = Localizer::new(&[tag(asked)]);
        let chain: Vec<String> = localizer
            .locales()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(chain, ["en-US"], "{asked}");
        assert_eq!(localizer.language(), tag("en-US"), "{asked}");
        assert_eq!(localizer.direction(), Direction::Ltr, "{asked}");
        let url = MsgArg::Text("http://127.0.0.1:9898".to_owned());
        let shown = localizer.message(msg::CLI_LISTENING, &[("url", url)]);
        assert_eq!(
            shown.text(),
            "listening on http://127.0.0.1:9898",
            "{asked}"
        );
    }
}
