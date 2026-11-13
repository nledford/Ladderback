//! A repository's further locations in the file (decision 80): read in
//! order with their own options, written back as tables of their own, and
//! refused where the file writes what can't be.

mod support;

use std::path::Path;

use app::ports::{ConfigStore, Problem};
use config_adapter::render;
use domain::RepositoryAlias;
use domain::planning::{
    AgentConfig, AgentSettings, Concurrency, LocationRule, MAX_RULE_ALTERNATIVES, NetworkKind,
    ValueKind, Violation,
};
use support::*;

const ID: &str = "4c6f636174696f6e73206f6620746865207265706f7369746f72792066697273";

/// A repository "nas" that records its ID, with `extra` after its keys.
fn nas(extra: &str) -> String {
    format!(
        "schema = 1\n\n[repositories.nas]\nrepository = \"/srv/lan\"\nrepository-id = \"{ID}\"\n\
         password-env = \"DEMO_PASSWORD\"\noptions = {{ http-cacert = \"/etc/ca.pem\" }}\n{extra}"
    )
}

fn location(repository: &str) -> String {
    format!("\n[[repositories.nas.locations]]\nrepository = \"{repository}\"\n")
}

#[test]
fn further_locations_are_read_in_order_with_their_own_options_and_written_back() {
    let text = nas(&format!(
        "{}options = {{ password = \"${{PUBLIC_KEY}}\" }}\n{}",
        location("rest:https://backup.example.net/r"),
        location("/srv/usb"),
    ));
    let config = parsed(&text).expect("read").config;
    let entry = &config.repositories()[&RepositoryAlias::parse("nas").expect("an alias")];
    let further: Vec<&str> = entry
        .locations
        .further
        .iter()
        .map(|location| location.repository.as_str())
        .collect();
    assert_eq!(further, ["rest:https://backup.example.net/r", "/srv/usb"]);
    assert_eq!(
        entry.locations.further[0]
            .options
            .get("password")
            .map(String::as_str),
        Some("${PUBLIC_KEY}")
    );
    assert!(
        !entry.locations.further[0]
            .options
            .contains_key("http-cacert"),
        "no option is inherited"
    );
    let written = render(Path::new(FILE), &config).expect("written");
    assert_eq!(written.matches("[[repositories.nas.locations]]").count(), 2);
    assert_eq!(parsed(&written).expect("read back").config, config);
}

#[test]
fn locations_on_a_repository_that_records_no_id_are_refused_at_their_table() {
    let text = format!(
        "schema = 1\n\n[repositories.nas]\nrepository = \"/srv/lan\"\npassword-env = \"PW\"\n{}",
        location("/srv/public")
    );
    let error = invalid(parsed(&text));
    assert_eq!(error.problem, Problem::MissingKey);
    assert_eq!(error.key.as_deref(), Some("repositories.nas.repository-id"));
    assert_eq!(
        error.at,
        Some(position_of(&text, "[[repositories.nas.locations]]"))
    );
}

#[test]
fn a_fifth_location_is_refused_at_its_table() {
    let text = nas(&["/srv/b", "/srv/c", "/srv/d", "/srv/e"]
        .map(location)
        .concat());
    let error = invalid(parsed(&text));
    assert_eq!(error.problem, Problem::Value(ValueKind::Location));
    assert_eq!(error.key.as_deref(), Some("repositories.nas.locations[3]"));
    let four = nas(&["/srv/b", "/srv/c", "/srv/d"].map(location).concat());
    assert!(parsed(&four).is_ok());
}

#[test]
fn a_further_location_is_checked_as_the_first_is() {
    for (extra, key, kind) in [
        (
            location("backups/repo"),
            "repositories.nas.locations[0].repository",
            ValueKind::LocalPath,
        ),
        (
            format!(
                "{}options = {{ root = \"backups\" }}\n",
                location("opendal:fs")
            ),
            "repositories.nas.locations[0].options.root",
            ValueKind::LocalPath,
        ),
        (
            // The first location's options are part of its place, so a
            // second at the same place repeats the earlier further one.
            format!("{}{}", location("/srv/public"), location("/srv/public")),
            "repositories.nas.locations[1].repository",
            ValueKind::Location,
        ),
    ] {
        let error = invalid(parsed(&nas(&extra)));
        assert_eq!(
            (error.key.as_deref(), error.problem),
            (Some(key), Problem::Value(kind)),
            "{extra}"
        );
    }
    // A location takes the keys of a location and no other.
    let error = invalid(parsed(&nas(&format!(
        "{}password = \"demo\"\n",
        location("/srv/public")
    ))));
    assert_eq!(error.problem, Problem::UnknownKey);
}

#[test]
fn two_entries_that_share_a_location_and_know_no_two_ids_are_one_repository() {
    let text = format!(
        "{}\n[repositories.usb]\nrepository = \"/srv/public\"\npassword-env = \"PW\"\n",
        nas(&location("/srv/public"))
    );
    let error = invalid(parsed(&text));
    assert!(
        matches!(
            error.problem,
            Problem::Violation(Violation::SameRepository { .. })
        ),
        "{:?}",
        error.problem
    );
    let other = text.replace(
        "[repositories.usb]\n",
        &format!(
            "[repositories.usb]\nrepository-id = \"{}\"\n",
            "0b".repeat(32)
        ),
    );
    assert!(parsed(&other).is_ok(), "two IDs say two repositories");
}

#[tokio::test]
async fn a_save_keeps_what_a_location_table_says_as_written() {
    let scratch = Scratch::new();
    let written = nas(&format!(
        "{}# the public address, away from home\noptions = {{ timeout = \"10s\" }}\n",
        location("rest:https://backup.example.net/r")
    ));
    scratch.write(&written);
    let store = scratch.store();
    let config = store.load().await.expect("loaded");
    // A change elsewhere in the file.
    let changed = AgentConfig::new(
        AgentSettings {
            concurrency: Concurrency::new(3).expect("a concurrency"),
            ..config.agent().clone()
        },
        config.hooks().clone(),
        config.repositories().clone(),
        config.plans().to_vec(),
    )
    .expect("a valid configuration");
    store.save(&changed).await.expect("saved");
    let saved = scratch.read();
    assert!(
        saved.contains("# the public address, away from home\n"),
        "{saved}"
    );
    assert_eq!(store.load().await.expect("loaded again"), changed);
}

#[test]
fn each_locations_rule_is_read_in_order_and_written_back() {
    let text = nas(&format!(
        "when = [ {{ network = \"wired\" }} ]\n{}when = [ {{ network = \"wifi\" }}, {{ network = \"wired\" }} ]\n{}",
        location("rest:https://backup.example.net/r"),
        location("/srv/usb"),
    ));
    let config = parsed(&text).expect("read").config;
    let entry = &config.repositories()[&RepositoryAlias::parse("nas").expect("an alias")];
    let kinds = |rule: Option<&LocationRule>| rule.map(|rule| rule.alternatives().to_vec());
    assert_eq!(
        kinds(entry.locations.first_when.as_ref()),
        Some(vec![NetworkKind::Wired])
    );
    assert_eq!(
        kinds(entry.locations.further[0].when.as_ref()),
        Some(vec![NetworkKind::WiFi, NetworkKind::Wired])
    );
    assert_eq!(kinds(entry.locations.further[1].when.as_ref()), None);
    let written = render(Path::new(FILE), &config).expect("written");
    assert_eq!(parsed(&written).expect("read back").config, config);
}

#[test]
fn a_rule_on_a_repository_of_one_location_needs_no_recorded_id() {
    let text = "schema = 1\n\n[repositories.solo]\nrepository = \"/srv/solo\"\npassword-env = \"PW\"\nwhen = [ { network = \"wifi\" } ]\n";
    let config = parsed(text).expect("read").config;
    let entry = &config.repositories()[&RepositoryAlias::parse("solo").expect("an alias")];
    assert!(entry.locations.first_when.is_some());
    assert!(entry.locations.further.is_empty());
}

#[test]
fn a_rule_that_is_empty_too_long_or_of_an_unknown_network_is_refused_at_its_key() {
    let nine = vec!["{ network = \"wifi\" }"; MAX_RULE_ALTERNATIVES + 1].join(", ");
    for (extra, key) in [
        ("when = []\n".to_owned(), "repositories.nas.when"),
        (format!("when = [ {nine} ]\n"), "repositories.nas.when"),
        (
            "when = [ { network = \"ethernet\" } ]\n".to_owned(),
            "repositories.nas.when[0].network",
        ),
        (
            format!(
                "{}when = [ {{ network = \"Wired\" }} ]\n",
                location("/srv/public")
            ),
            "repositories.nas.locations[0].when[0].network",
        ),
    ] {
        let error = invalid(parsed(&nas(&extra)));
        assert_eq!(
            (error.key.as_deref(), error.problem),
            (Some(key), Problem::Value(ValueKind::LocationRule)),
            "{extra}"
        );
    }
    // An alternative takes `network` and, from part 3, its lists: nothing
    // else.
    let error = invalid(parsed(&nas(
        "when = [ { network = \"wifi\", ssid = \"Office\" } ]\n",
    )));
    assert_eq!(error.problem, Problem::UnknownKey);
    let error = invalid(parsed(&nas("when = [ { } ]\n")));
    assert_eq!(error.problem, Problem::MissingKey);
}
