use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use domain::execution::plan_tags::{PlanTags, UserTag};
use domain::planning::{
    GroupBy, Hooks, KeepCount, Maintenance, Plan, RepositoryConfig, RepositorySettings,
    RetentionPolicy, Source,
};
use domain::secrets::{FileRead, Part, SecretRef, SecretValue, Template};
use domain::{AgentId, PlanId, RepositoryAlias, Uuid};

use crate::ports::{
    ExportRefusal, PlanExport, ProfileError, ProfileNotice, ProfileProblem, ProfileReader,
    ProfileWriter,
};

/// Sets up the profile files the reader reads, in memory for a fake and for
/// real in a scratch folder for the real reader, so the same checks run
/// against both. A method that makes a profile returns where it is, and the
/// profile is for the suite to read before it asks for the next: a fake
/// holds one at a time.
pub trait ProfilesHarness: Send + Sync {
    fn reader(&self) -> Arc<dyn ProfileReader>;

    fn writer(&self) -> Arc<dyn ProfileWriter>;

    /// A profile of one plan: `home`, which backs up `/home/demo` into the
    /// repository at `/srv/demo-repo`, whose password is in the file
    /// `/srv/demo-password`. The reader will be asked for `repository`.
    fn good_profile(&self, repository: &RepositoryAlias) -> PathBuf;

    /// A profile as `good_profile`, whose repository is written as
    /// `s3:${file:/srv/demo-endpoint}`: text that the config would read as a
    /// reference to a file.
    fn reference_profile(&self, repository: &RepositoryAlias) -> PathBuf;

    /// A file that is not a profile: its text is not TOML.
    fn broken_profile(&self) -> PathBuf;

    /// A path with nothing at it, in a folder that exists.
    fn missing_profile(&self) -> PathBuf;
}

/// What the suite checks of the reader: every plan is for the repository it
/// was asked for, what the profile wrote stays text, and a file it can't read
/// is a [`ProfileError`] that names it. Of the writer: the text names the
/// plan, a secret is never in it, and what it can't say is refused or noted.
pub async fn run(harness: &dyn ProfilesHarness) {
    a_profile_gives_its_plans_for_the_repository_asked(harness).await;
    a_reference_a_profile_wrote_stays_text(harness).await;
    a_file_that_is_not_a_profile_is_refused(harness).await;
    a_file_that_is_not_there_is_unreadable(harness).await;
    a_plan_is_written_under_its_id(harness);
    the_same_plan_is_written_the_same_way_twice(harness);
    a_password_in_the_config_is_refused_and_never_written(harness);
    a_location_that_is_a_reference_is_refused(harness);
    an_option_that_is_a_reference_or_a_secret_is_left_out_with_a_variable(harness);
    a_plan_that_keeps_everything_is_written_with_a_notice(harness);
    a_location_with_a_password_in_its_url_is_refused_and_never_written(harness);
    a_tag_with_a_comma_is_refused(harness);
}

fn alias(text: &str) -> Option<RepositoryAlias> {
    RepositoryAlias::parse(text).ok()
}

async fn a_profile_gives_its_plans_for_the_repository_asked(harness: &dyn ProfilesHarness) {
    let Some(demo) = alias("demo") else {
        return;
    };
    let file = harness.good_profile(&demo);
    let read = harness.reader().read(&file, &demo).await;
    assert!(read.is_ok(), "a good profile is read: {read:?}");
    let Ok(profile) = read else { return };
    assert_eq!(profile.plans.len(), 1, "{profile:?}");
    for plan in &profile.plans {
        assert_eq!(plan.id.as_str(), "home");
        assert_eq!(
            plan.repository, demo,
            "every plan backs up into the repository it was asked for"
        );
        let sources: Vec<_> = plan.sources.iter().map(Source::as_str).collect();
        assert_eq!(sources, ["/home/demo"]);
    }
    assert_eq!(profile.repository.settings.repository, "/srv/demo-repo");
    assert_eq!(
        profile.repository.settings.password,
        SecretRef::File {
            path: "/srv/demo-password".to_owned(),
            read: FileRead::FirstLine,
        }
    );
    // The same file, read again, says the same.
    let again = harness.reader().read(&file, &demo).await;
    assert_eq!(again.as_ref(), Ok(&profile), "a read changes nothing");
}

async fn a_reference_a_profile_wrote_stays_text(harness: &dyn ProfilesHarness) {
    let Some(demo) = alias("demo") else {
        return;
    };
    let file = harness.reference_profile(&demo);
    let read = harness.reader().read(&file, &demo).await;
    assert!(read.is_ok(), "the profile is read: {read:?}");
    let Ok(profile) = read else { return };
    let repository = &profile.repository.settings.repository;
    let template = Template::parse(repository);
    assert!(
        template.is_ok(),
        "the repository is a template the config reads"
    );
    let Ok(template) = template else { return };
    assert!(
        template
            .parts()
            .iter()
            .all(|part| matches!(part, Part::Text(_))),
        "what a profile wrote is text, never a reference: {template:?}"
    );
    let text: String = template
        .parts()
        .iter()
        .map(|part| match part {
            Part::Text(text) => text.as_str(),
            Part::Reference(_) => "",
        })
        .collect();
    assert_eq!(text, "s3:${file:/srv/demo-endpoint}");
}

async fn a_file_that_is_not_a_profile_is_refused(harness: &dyn ProfilesHarness) {
    let Some(demo) = alias("demo") else {
        return;
    };
    let file = harness.broken_profile();
    let read = harness.reader().read(&file, &demo).await;
    assert!(
        matches!(
            &read,
            Err(ProfileError::Invalid {
                problem: ProfileProblem::Syntax,
                ..
            })
        ),
        "a file that is not TOML is invalid: {read:?}"
    );
    if let Err(ProfileError::Invalid { file: named, .. }) = read {
        assert_eq!(named, file.display().to_string(), "the file is named");
    }
}

async fn a_file_that_is_not_there_is_unreadable(harness: &dyn ProfilesHarness) {
    let Some(demo) = alias("demo") else {
        return;
    };
    let file = harness.missing_profile();
    let read = harness.reader().read(&file, &demo).await;
    assert!(
        matches!(&read, Err(ProfileError::Unreadable { .. })),
        "a missing file is unreadable: {read:?}"
    );
    if let Err(ProfileError::Unreadable { file: named }) = read {
        assert_eq!(named, file.display().to_string(), "the file is named");
    }
}

// --- The writer ---------------------------------------------------------------

/// A plan `home` of `/home/demo` in the repository `demo` at `location`,
/// whose password is in a file, for an agent with no hostname and no hooks.
fn export(location: &str) -> Option<PlanExport> {
    let plan_id = PlanId::parse("home").ok()?;
    let mut plan = Plan::new(
        plan_id.clone(),
        alias("demo")?,
        vec![Source::parse("/home/demo").ok()?],
    );
    plan.retention.keep_last = KeepCount::new(3).ok();
    Some(PlanExport {
        plan,
        repository: RepositoryConfig {
            locations: Default::default(),
            id: None,
            settings: RepositorySettings {
                repository: location.to_owned(),
                repo_hot: None,
                password: SecretRef::File {
                    path: "/srv/demo-password".to_owned(),
                    read: FileRead::FirstLine,
                },
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance: Maintenance::None,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        },
        agent_hooks: Hooks::default(),
        agent: domain::planning::AgentSettings::default(),
        tags: PlanTags::new(plan_id, AgentId::from_uuid(Uuid::from_u128(1))),
    })
}

fn a_plan_is_written_under_its_id(harness: &dyn ProfilesHarness) {
    let Some(export) = export("/srv/demo-repo") else {
        return;
    };
    let written = harness.writer().render(&export);
    assert!(written.is_ok(), "a plan is written: {written:?}");
    let Ok(profile) = written else { return };
    assert!(profile.text.contains("home"), "{}", profile.text);
    assert!(profile.text.ends_with('\n'), "{:?}", profile.text);
    assert!(profile.notices.is_empty(), "{:?}", profile.notices);
}

fn the_same_plan_is_written_the_same_way_twice(harness: &dyn ProfilesHarness) {
    let Some(export) = export("/srv/demo-repo") else {
        return;
    };
    let first = harness.writer().render(&export);
    let second = harness.writer().render(&export);
    assert!(first.is_ok());
    assert_eq!(first, second, "a profile is stable");
}

fn a_password_in_the_config_is_refused_and_never_written(harness: &dyn ProfilesHarness) {
    let Some(mut export) = export("/srv/demo-repo") else {
        return;
    };
    export.repository.settings.password =
        SecretRef::Inline(SecretValue::new("CANARY-inline-password"));
    let written = harness.writer().render(&export);
    assert_eq!(
        written.as_ref().err(),
        Some(&ExportRefusal::InlineSecret),
        "{written:?}"
    );
    let shown = format!("{written:?}");
    assert!(!shown.contains("CANARY"), "{shown}");
}

fn a_location_that_is_a_reference_is_refused(harness: &dyn ProfilesHarness) {
    for (location, hot, key) in [
        ("${DEMO_REPOSITORY}", None, "repository"),
        (
            "/srv/demo-repo",
            Some("s3:${file:/srv/demo-hot}"),
            "repo-hot",
        ),
    ] {
        let Some(mut export) = export(location) else {
            return;
        };
        export.repository.settings.repo_hot = hot.map(str::to_owned);
        let written = harness.writer().render(&export);
        assert_eq!(
            written.as_ref().err(),
            Some(&ExportRefusal::Reference {
                key: key.to_owned()
            }),
            "{location} {hot:?}: {written:?}"
        );
    }
}

fn an_option_that_is_a_reference_or_a_secret_is_left_out_with_a_variable(
    harness: &dyn ProfilesHarness,
) {
    let Some(mut export) = export("/srv/demo-repo") else {
        return;
    };
    for (name, value) in [
        ("region", "eu-demo-1"),
        ("token", "${DEMO_TOKEN}"),
        ("secret_access_key", "CANARY-option"),
        // Credentials in an address that has no scheme, or only a user.
        ("proxy", "demo:CANARY-proxy@proxy.demo:3128"),
        ("other_proxy", "CANARY-token@proxy.demo:3128"),
    ] {
        export
            .repository
            .settings
            .options
            .insert(name.to_owned(), value.to_owned());
    }
    let written = harness.writer().render(&export);
    assert!(written.is_ok(), "the export is made: {written:?}");
    let Ok(profile) = written else { return };
    assert!(
        !profile.text.contains("CANARY") && !profile.text.contains("DEMO_TOKEN"),
        "{}",
        profile.text
    );
    let mut left_out: Vec<_> = profile
        .notices
        .iter()
        .filter_map(|notice| match notice {
            ProfileNotice::OptionLeftOut { option, variable } => {
                assert!(!variable.is_empty(), "{notice:?}");
                Some(option.as_str())
            }
            _ => None,
        })
        .collect();
    left_out.sort_unstable();
    assert_eq!(
        left_out,
        ["other_proxy", "proxy", "secret_access_key", "token"]
    );
}

fn a_plan_that_keeps_everything_is_written_with_a_notice(harness: &dyn ProfilesHarness) {
    let Some(mut export) = export("/srv/demo-repo") else {
        return;
    };
    export.plan.retention = RetentionPolicy::default();
    let written = harness.writer().render(&export);
    assert!(written.is_ok(), "the export is made: {written:?}");
    let Ok(profile) = written else { return };
    assert!(
        profile.notices.contains(&ProfileNotice::RetentionEmpty),
        "rustic's forget will refuse the profile, and the export says so: {:?}",
        profile.notices
    );
}

fn a_location_with_a_password_in_its_url_is_refused_and_never_written(
    harness: &dyn ProfilesHarness,
) {
    for (location, hot, key) in [
        (
            "rest:https://demo:CANARY-url@nas.demo/repo",
            None,
            "repository",
        ),
        (
            "/srv/demo-repo",
            Some("rest:https://demo:CANARY-url@hot.demo/"),
            "repo-hot",
        ),
        (
            "rclone::s3,secret_access_key=CANARY-url:bucket/repo",
            None,
            "repository",
        ),
        (
            "/srv/demo-repo",
            Some("rclone::drive,token=CANARY-url:repo"),
            "repo-hot",
        ),
    ] {
        let Some(mut export) = export(location) else {
            return;
        };
        export.repository.settings.repo_hot = hot.map(str::to_owned);
        let written = harness.writer().render(&export);
        assert_eq!(
            written.as_ref().err(),
            Some(&ExportRefusal::UrlPassword {
                key: key.to_owned()
            }),
            "{key}: {written:?}"
        );
        let shown = format!("{written:?}");
        assert!(!shown.contains("CANARY"), "{shown}");
    }
}

fn a_tag_with_a_comma_is_refused(harness: &dyn ProfilesHarness) {
    let Some(mut export) = export("/srv/demo-repo") else {
        return;
    };
    let Ok(tag) = UserTag::new("a,b") else {
        return;
    };
    export.plan.tags = vec![tag];
    let written = harness.writer().render(&export);
    assert_eq!(
        written.as_ref().err(),
        Some(&ExportRefusal::TagComma {
            tag: "a,b".to_owned()
        }),
        "{written:?}"
    );
}
