use super::support::*;
use super::*;

fn field_warning(field: &str) -> api::WarningDto {
    api::WarningDto {
        code: WarningCode::ConfigInlineSecret,
        args: [("field".to_owned(), MsgArg::Text(field.to_owned()))].into(),
    }
}

#[test]
fn a_listed_repository_with_secrets_in_the_config_carries_a_warning_for_each_without_the_value() {
    let (password, key) = ("not-a-real-password", "not-a-real-key");
    let mut settings = demo_settings(SecretRef::Inline(SecretValue::new(password)));
    settings
        .options_cold
        .insert("secret_access_key".to_owned(), key.to_owned());
    let alias = RepositoryAlias::parse("demo").expect("an alias");
    let row = repository_row(
        &alias,
        &settings,
        domain::planning::Maintenance::Local,
        false,
    );
    assert_eq!(
        row.warnings,
        [
            field_warning("password"),
            field_warning("options-cold.secret_access_key")
        ]
    );
    let wire = serde_json::to_string(&row).expect("a row is JSON");
    assert!(
        !wire.contains(password) && !wire.contains(key),
        "the row repeats a secret: {wire}"
    );
}

#[test]
fn a_listed_repository_whose_secrets_are_read_from_elsewhere_has_no_warning() {
    let mut settings = demo_settings(SecretRef::Env("DEMO_PASSWORD".to_owned()));
    settings
        .options
        .insert("secret_access_key".to_owned(), "${DEMO_KEY}".to_owned());
    let alias = RepositoryAlias::parse("demo").expect("an alias");
    assert_eq!(
        repository_row(
            &alias,
            &settings,
            domain::planning::Maintenance::Local,
            false
        )
        .warnings,
        []
    );
}

#[test]
fn a_listed_repository_with_a_password_in_its_location_is_warned_of_by_the_setting_never_the_password()
 {
    let (cold, hot) = ("CANARY-cold-password", "CANARY-hot-password");
    let mut settings = demo_settings(SecretRef::Env("DEMO_PASSWORD".to_owned()));
    settings.repository = format!("rest:https://demo:{cold}@backup.example.com/nas");
    settings.repo_hot = Some(format!("rest:https://demo:{hot}@hot.example.com/nas"));
    let alias = RepositoryAlias::parse("demo").expect("an alias");
    let row = repository_row(
        &alias,
        &settings,
        domain::planning::Maintenance::Local,
        false,
    );
    let location = |field: &str| api::WarningDto {
        code: WarningCode::LocationSecret,
        args: [
            ("field".to_owned(), MsgArg::Text(field.to_owned())),
            ("kind".to_owned(), MsgArg::Text("url".to_owned())),
        ]
        .into(),
    };
    assert_eq!(row.warnings, [location("repository"), location("repo-hot")]);
    let wire = serde_json::to_string(&row).expect("a row is JSON");
    let shown = row_warnings(&row);
    // The row shows the location masked (`demo:***@`), but never the
    // password; the warning names only the setting, not the user either.
    assert!(
        !wire.contains(cold) && !wire.contains(hot),
        "the row repeats a password: {wire}"
    );
    for said in &shown {
        assert!(
            !said.contains(cold) && !said.contains(hot) && !said.contains("demo"),
            "a warning repeats the credentials: {said}"
        );
    }
    assert!(
        shown[0].contains("The address in the repository setting holds a password as text")
            && shown[1].contains("The address in the repo-hot setting holds a password as text"),
        "{shown:?}"
    );
}

/// rclone is handed the location in its arguments, where a `${…}`
/// reference is refused (§10.2), so the warning sends the credential to
/// rclone's own configuration file instead.
#[test]
fn a_listed_repository_with_a_credential_in_its_rclone_location_is_sent_to_rclones_configuration() {
    let pass = "CANARY-rclone-pass";
    let mut settings = demo_settings(SecretRef::Env("DEMO_PASSWORD".to_owned()));
    settings.repository = format!("rclone::sftp,host=nas.example.com,pass={pass}:/backups");
    let alias = RepositoryAlias::parse("demo").expect("an alias");
    let row = repository_row(
        &alias,
        &settings,
        domain::planning::Maintenance::Local,
        false,
    );
    assert_eq!(
        row.warnings,
        [api::WarningDto {
            code: WarningCode::LocationSecret,
            args: [
                ("field".to_owned(), MsgArg::Text("repository".to_owned())),
                ("kind".to_owned(), MsgArg::Text("rclone".to_owned())),
            ]
            .into(),
        }]
    );
    let shown = row_warnings(&row);
    assert!(
        shown[0].contains("rclone's own configuration file")
            && shown[0].contains("rclone:remote:path"),
        "{shown:?}"
    );
    assert!(
        !shown[0].contains("${") && !shown[0].contains(pass),
        "{shown:?}"
    );
}

#[test]
fn a_listed_repository_shows_its_rclone_locations_without_the_credentials_written_in_them() {
    let mut settings = demo_settings(SecretRef::Env("DEMO_PASSWORD".to_owned()));
    settings.repository =
        "rclone::sftp,host=nas.example.com,pass='CANARY-rclone-cold,x:y':/backups".to_owned();
    settings.repo_hot =
        Some("rclone::b2,account=demo,key=CANARY-rclone-hot:demo-bucket".to_owned());
    let alias = RepositoryAlias::parse("demo").expect("an alias");
    let row = repository_row(
        &alias,
        &settings,
        domain::planning::Maintenance::Local,
        false,
    );
    assert_eq!(
        row.repository,
        "rclone::sftp,host=nas.example.com,pass=***:/backups"
    );
    assert_eq!(
        row.repo_hot.as_deref(),
        Some("rclone::b2,account=demo,key=***:demo-bucket")
    );
    let wire = serde_json::to_string(&row).expect("a row is JSON");
    assert!(
        !wire.contains("CANARY") && !wire.contains("x:y"),
        "the row repeats a credential: {wire}"
    );
}

/// What a listed repository's warnings say, in English.
fn row_warnings(row: &api::RepositoryDto) -> Vec<String> {
    row.warnings
        .iter()
        .map(|warning| {
            let args: Vec<(&str, MsgArg)> = warning
                .args
                .iter()
                .map(|(name, value)| (name.as_str(), value.clone()))
                .collect();
            english()
                .warning(warning.code, &args, None)
                .text()
                .to_owned()
        })
        .collect()
}

#[test]
fn a_listed_repository_whose_location_has_no_password_written_in_it_has_no_warning() {
    let mut settings = demo_settings(SecretRef::Env("DEMO_PASSWORD".to_owned()));
    for location in [
        "rest:https://demo@backup.example.com/nas",
        "rest:https://demo:${DEMO_REST_PASSWORD}@backup.example.com/nas",
    ] {
        settings.repository = location.to_owned();
        let alias = RepositoryAlias::parse("demo").expect("an alias");
        assert_eq!(
            repository_row(
                &alias,
                &settings,
                domain::planning::Maintenance::Local,
                false
            )
            .warnings,
            [],
            "{location}"
        );
    }
}

#[test]
fn a_listed_repository_says_whether_this_agent_maintains_it() {
    let settings = demo_settings(SecretRef::Env("DEMO_PASSWORD".to_owned()));
    let alias = RepositoryAlias::parse("demo").expect("an alias");
    for (maintenance, here) in [
        (domain::planning::Maintenance::Local, true),
        (domain::planning::Maintenance::Hub, false),
        (domain::planning::Maintenance::None, false),
    ] {
        let row = repository_row(&alias, &settings, maintenance, false);
        assert_eq!(row.maintained_here, here, "{maintenance:?}");
        let wire = serde_json::to_value(&row).expect("a row is JSON");
        assert_eq!(
            wire["maintained-here"],
            serde_json::Value::Bool(here),
            "{maintenance:?}"
        );
    }
}

/// An entry `nas` reached at `rest:http://lan.example.lan/r` with a user
/// and password written in it and a key by reference, then at
/// `/srv/public` with a key written in the file.
fn two_locations() -> RepositoryConfig {
    let mut first = demo_settings(SecretRef::Env("NAS_PASSWORD".to_owned()));
    first.repository = "rest:http://lan:CANARY-url-pass@lan.example.lan/r".to_owned();
    first
        .options
        .insert("application_key".to_owned(), "${LAN_KEY}".to_owned());
    RepositoryConfig {
        locations: domain::planning::Locations {
            first_when: None,
            further: vec![Location {
                repository: "/srv/public".to_owned(),
                options_cold: [(
                    "secret_access_key".to_owned(),
                    "CANARY-written-key".to_owned(),
                )]
                .into(),
                ..Location::default()
            }],
        },
        id: None,
        settings: first,
        maintenance: Maintenance::Local,
        group_by: Default::default(),
        hooks: Default::default(),
    }
}

#[test]
fn a_listed_repository_lists_its_locations_in_order_with_references_and_never_a_written_secret() {
    let alias = RepositoryAlias::parse("nas").expect("an alias");
    let row = listed_repository(&alias, &two_locations(), &[], true, false);
    let option = |set: &str, name: &str, reference: Option<&str>| api::LocationOptionDto {
        set: set.to_owned(),
        name: name.to_owned(),
        reference: reference.map(str::to_owned),
    };
    assert_eq!(
        row.locations,
        vec![
            api::LocationDto {
                position: 1,
                repository: "rest:http://lan:***@lan.example.lan/r".to_owned(),
                repo_hot: None,
                options: vec![option("options", "application_key", Some("${LAN_KEY}"))],
                when: Vec::new(),
            },
            api::LocationDto {
                position: 2,
                repository: "/srv/public".to_owned(),
                repo_hot: None,
                options: vec![option("options-cold", "secret_access_key", None)],
                when: Vec::new(),
            },
        ]
    );
    let wire = serde_json::to_string(&row).expect("a row is JSON");
    assert!(!wire.contains("CANARY"), "the row shows a secret: {wire}");
}

#[test]
fn a_listed_repository_warns_of_a_plain_http_location_and_says_whether_it_has_a_rule() {
    let alias = RepositoryAlias::parse("nas").expect("an alias");
    let warning = |rule: &str| api::WarningDto {
        code: WarningCode::LocationPlainHttpCredentials,
        args: [
            ("position".to_owned(), MsgArg::Text("1".to_owned())),
            ("cause".to_owned(), MsgArg::Text("plain-http".to_owned())),
            ("rule".to_owned(), MsgArg::Text(rule.to_owned())),
            ("locations".to_owned(), MsgArg::Text("several".to_owned())),
        ]
        .into(),
    };
    let mut entry = two_locations();
    let row = listed_repository(&alias, &entry, &[], true, false);
    assert!(
        row.warnings.contains(&warning("none")),
        "{:?}",
        row.warnings
    );
    entry.locations.first_when =
        Some(LocationRule::new(vec![domain::planning::NetworkKind::Wired.into()]).expect("a rule"));
    let row = listed_repository(&alias, &entry, &[], true, false);
    assert!(
        row.warnings.contains(&warning("network")),
        "{:?}",
        row.warnings
    );
}

#[test]
fn a_listed_repository_shows_each_locations_rule_and_notes_what_the_rules_mean() {
    let alias = RepositoryAlias::parse("nas").expect("an alias");
    let mut entry = two_locations();
    let wired =
        LocationRule::new(vec![domain::planning::NetworkKind::Wired.into()]).expect("a rule");
    entry.locations.first_when = Some(wired.clone());
    entry.locations.further[0].when = Some(
        LocationRule::new(vec![
            domain::planning::NetworkKind::WiFi.into(),
            domain::planning::NetworkKind::Wired.into(),
        ])
        .expect("a rule"),
    );
    let mut plan = Plan::new(
        PlanId::parse("home").expect("a plan ID"),
        alias.clone(),
        Vec::new(),
    );
    plan.schedule = Some(domain::planning::Schedule::new(
        domain::planning::Interval::parse("1d").expect("an interval"),
    ));
    let row = listed_repository(&alias, &entry, &[&plan], true, false);
    let when: Vec<Vec<&str>> = row
        .locations
        .iter()
        .map(|location| {
            location
                .when
                .iter()
                .map(|kind| kind.network.as_str())
                .collect()
        })
        .collect();
    assert_eq!(when, vec![vec!["wired"], vec!["wifi", "wired"]]);
    let note = |code, args: &[(&str, &str)]| api::WarningDto {
        code,
        args: args
            .iter()
            .map(|(name, value)| ((*name).to_owned(), MsgArg::Text((*value).to_owned())))
            .collect(),
    };
    for expected in [
        note(WarningCode::LocationEveryRuled, &[("locations", "several")]),
        note(
            WarningCode::LocationLastRuledNoRetry,
            &[("plan", "home"), ("locations", "several")],
        ),
    ] {
        assert!(
            row.warnings.contains(&expected),
            "{expected:?} in {:?}",
            row.warnings
        );
    }
    assert!(
        listed_repository(&alias, &entry, &[], true, false)
            .warnings
            .iter()
            .all(|warning| warning.code != WarningCode::LocationLastRuledNoRetry),
        "no plan: no note about retries"
    );
}

#[test]
fn a_listed_rule_names_its_networks_and_is_noted_where_names_cant_be_read() {
    use domain::planning::{Alternative, NetworkKind, NetworkName};
    let alias = RepositoryAlias::parse("nas").expect("an alias");
    let names = |names: &[&str]| {
        names
            .iter()
            .map(|name| NetworkName::parse(name).expect("a name"))
            .collect()
    };
    let wifi = |filter| {
        LocationRule::new(vec![
            Alternative::new(NetworkKind::WiFi, filter).expect("an alternative"),
        ])
        .expect("a rule")
    };
    let mut entry = two_locations();
    entry.locations.first_when = Some(wifi(Some(NameFilter::Only(names(&["Office", "Lab"])))));
    entry.locations.further[0].when = Some(wifi(Some(NameFilter::Except(names(&["Guest"])))));
    let row = listed_repository(&alias, &entry, &[], false, false);
    let when: Vec<(Vec<String>, Vec<String>)> = row
        .locations
        .iter()
        .flat_map(|location| location.when.iter())
        .map(|alternative| (alternative.only.clone(), alternative.except.clone()))
        .collect();
    let owned = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(
        when,
        vec![
            (owned(&["Office", "Lab"]), Vec::new()),
            (Vec::new(), owned(&["Guest"])),
        ]
    );
    let unreadable: Vec<&api::WarningDto> = row
        .warnings
        .iter()
        .filter(|warning| warning.code == WarningCode::LocationRuleNameUnreadable)
        .collect();
    assert_eq!(
        unreadable
            .iter()
            .map(|warning| warning.args.get("position"))
            .collect::<Vec<_>>(),
        [
            Some(&MsgArg::Text("1".to_owned())),
            Some(&MsgArg::Text("2".to_owned())),
        ]
    );
    assert!(
        listed_repository(&alias, &entry, &[], true, false)
            .warnings
            .iter()
            .all(|warning| warning.code != WarningCode::LocationRuleNameUnreadable),
        "a machine that reads the name: no note"
    );
    let wire = serde_json::to_value(&row).expect("a row is JSON");
    assert_eq!(
        wire.pointer("/locations/0/when/0/except"),
        None,
        "no empty list on the wire: {wire}"
    );
}

#[test]
fn a_listed_repository_of_one_location_lists_no_locations() {
    let alias = RepositoryAlias::parse("demo").expect("an alias");
    let mut entry = two_locations();
    entry.locations = Default::default();
    let row = listed_repository(&alias, &entry, &[], true, false);
    assert_eq!(row.locations, Vec::new());
    let wire = serde_json::to_value(&row).expect("a row is JSON");
    assert_eq!(wire.get("locations"), None, "{wire}");
}

/// The note names each family's kind of link, by its code, and says of
/// a repository of one location that it is one; an entry with no rule,
/// a link of one interface and a link not read are not noted.
#[test]
fn a_mixed_link_is_noted_by_the_kind_of_each_family_s_link() {
    use domain::planning::NetworkKind;
    let mixed = Some(Link::Mixed {
        ipv4: InterfaceKind::WiFi,
        ipv6: InterfaceKind::Other,
    });
    let mut entry = two_locations();
    assert_eq!(mixed_link_note(&entry, mixed), None, "no rule");
    entry.locations.further[0].when =
        Some(LocationRule::new(vec![NetworkKind::Wired.into()]).expect("a rule"));
    let args = |locations: &str| -> std::collections::BTreeMap<String, MsgArg> {
        [
            ("ipv4".to_owned(), MsgArg::Text("wifi".to_owned())),
            ("ipv6".to_owned(), MsgArg::Text("other".to_owned())),
            ("locations".to_owned(), MsgArg::Text(locations.to_owned())),
        ]
        .into()
    };
    assert_eq!(
        mixed_link_note(&entry, mixed),
        Some(api::WarningDto {
            code: WarningCode::LocationRuleMixedLink,
            args: args("several"),
        })
    );
    for link in [Some(Link::WiFi), Some(Link::NoRoute), None] {
        assert_eq!(mixed_link_note(&entry, link), None, "{link:?}");
    }
    entry.locations.first_when = entry.locations.further[0].when.clone();
    entry.locations.further.clear();
    assert_eq!(
        mixed_link_note(&entry, mixed).map(|note| note.args),
        Some(args("one"))
    );
}

/// The page lists no locations for a repository of one, so neither its
/// warning nor its notes name a location by its position or a rule the
/// page doesn't show (#473), and the warning is given with no rule as
/// with one (#506).
#[test]
fn a_listed_repository_of_one_location_is_warned_and_noted_as_a_repository() {
    use domain::planning::{Alternative, NetworkKind, NetworkName};
    let alias = RepositoryAlias::parse("nas").expect("an alias");
    let mut entry = two_locations();
    entry.locations = Default::default();
    let row = listed_repository(&alias, &entry, &[], true, false);
    let exposed = api::WarningDto {
        code: WarningCode::LocationPlainHttpCredentials,
        args: [
            ("position".to_owned(), MsgArg::Text("1".to_owned())),
            ("cause".to_owned(), MsgArg::Text("plain-http".to_owned())),
            ("rule".to_owned(), MsgArg::Text("none".to_owned())),
            ("locations".to_owned(), MsgArg::Text("one".to_owned())),
        ]
        .into(),
    };
    assert!(row.warnings.contains(&exposed), "{:?}", row.warnings);

    let office = NetworkName::parse("Office").expect("a name");
    entry.locations.first_when = Some(
        LocationRule::new(vec![
            Alternative::new(NetworkKind::WiFi, Some(NameFilter::Only(vec![office])))
                .expect("an alternative"),
        ])
        .expect("a rule"),
    );
    let mut plan = Plan::new(
        PlanId::parse("home").expect("a plan ID"),
        alias.clone(),
        Vec::new(),
    );
    plan.schedule = Some(domain::planning::Schedule::new(
        domain::planning::Interval::parse("1d").expect("an interval"),
    ));
    let row = listed_repository(&alias, &entry, &[&plan], false, false);
    for code in [
        WarningCode::LocationPlainHttpCredentials,
        WarningCode::LocationEveryRuled,
        WarningCode::LocationLastRuledNoRetry,
        WarningCode::LocationRuleNameUnreadable,
    ] {
        let warning = row
            .warnings
            .iter()
            .find(|warning| warning.code == code)
            .unwrap_or_else(|| panic!("no {code:?} in {:?}", row.warnings));
        assert_eq!(
            warning.args.get("locations"),
            Some(&MsgArg::Text("one".to_owned())),
            "{code:?}"
        );
    }
}

/// A secret written as text in a further location is pointed out as one
/// in the first is, by the setting and the location's position, never
/// the secret (#505).
#[test]
fn a_listed_repository_points_out_a_secret_written_in_a_further_location() {
    let alias = RepositoryAlias::parse("nas").expect("an alias");
    let mut entry = two_locations();
    entry.locations.further[0].repo_hot =
        Some("rest:https://demo:CANARY-hot-pass@hot.example.lan/r".to_owned());
    let row = listed_repository(&alias, &entry, &[], true, false);
    let warning = |code, args: &[(&str, &str)]| api::WarningDto {
        code,
        args: args
            .iter()
            .map(|(name, value)| ((*name).to_owned(), MsgArg::Text((*value).to_owned())))
            .collect(),
    };
    for expected in [
        warning(
            WarningCode::LocationSecret,
            &[("field", "repository"), ("kind", "url")],
        ),
        warning(
            WarningCode::LocationSecret,
            &[("field", "repo-hot"), ("kind", "url"), ("position", "2")],
        ),
        warning(
            WarningCode::ConfigInlineSecret,
            &[
                ("field", "options-cold.secret_access_key"),
                ("position", "2"),
            ],
        ),
    ] {
        assert!(
            row.warnings.contains(&expected),
            "{expected:?} in {:?}",
            row.warnings
        );
    }
    let shown = format!("{row:?}");
    for secret in ["CANARY-url-pass", "CANARY-written-key", "CANARY-hot-pass"] {
        assert!(!shown.contains(secret), "{shown}");
    }
}

#[test]
fn an_operations_summary_carries_the_locations_its_choice_saw() {
    use domain::execution::locations::LocationTried;
    let tried = |position, address: &str, outcome| LocationTried {
        position,
        address: address.to_owned(),
        outcome,
        source: None,
    };
    let record = LocationsRecord {
        locations: vec![
            tried(1, "/srv/lan", LocationOutcome::Silent),
            tried(2, "/srv/public", LocationOutcome::Used),
        ],
    };
    let summary = api::OperationSummaryDto::new(
        "018f0000-0000-7000-8000-000000000001".to_owned(),
        "home".to_owned(),
        "backup".to_owned(),
        "succeeded".to_owned(),
        "2026-11-12T10:00:00Z".to_owned(),
        None,
        None,
        Vec::new(),
        None,
        None,
        None,
    )
    .expect("a summary")
    .with_locations(locations_tried(&record));
    assert_eq!(
        summary
            .used_location()
            .map(|(used, count)| (used.position, used.address.as_str(), count)),
        Some((2, "/srv/public", 2))
    );
    let wire = serde_json::to_value(&summary).expect("a summary is JSON");
    assert_eq!(
        wire["locations"],
        serde_json::json!([
            {"position": 1, "address": "/srv/lan", "outcome": "silent"},
            {"position": 2, "address": "/srv/public", "outcome": "used"},
        ])
    );
}
