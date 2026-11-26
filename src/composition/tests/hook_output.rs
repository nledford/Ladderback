use super::captured_log::{self, capture};
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use app::fakes::InMemoryConfigStore;

use super::service_support::*;
use super::*;

use app::ports::HookOutcome;

/// The repository's password, which the run reads from a file.
const HOOK_PASSWORD: &str = "canary-hook/pass@word";
/// A credential the run's repository holds as text, in its options.
const HOOK_INLINE: &str = "canary-inline\"key";
/// The password another repository of the configuration holds as text.
const HOOK_OTHER: &str = "canary-other-repo";

/// The real hook runner, keeping what each hook it ran wrote as the run
/// got it.
struct KeptHooks {
    runner: ProcessHookRunner,
    outcomes: Mutex<Vec<HookOutcome>>,
}

#[async_trait]
impl HookRunner for KeptHooks {
    async fn run(
        &self,
        entry: &domain::planning::HookEntry,
        context: &app::ports::HookContext,
        redactor: &domain::secrets::Redactor,
    ) -> HookOutcome {
        let outcome = self.runner.run(entry, context, redactor).await;
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(outcome.clone());
        outcome
    }

    fn environment_secrets(&self) -> Vec<domain::secrets::SecretValue> {
        self.runner.environment_secrets()
    }
}

/// What reached the hooks' output, the history's row and the daemon's
/// log when the plan `home` ran by hand with the hooks `agent` and
/// `plan`, each a hook that prints the run's secrets in the forms a
/// redactor knows (§10.2): the repository's password, read from a file
/// as the run starts, and two secrets the configuration holds as text,
/// one the run's repository's and one another repository's. `fails`
/// makes the engine fail the backup. With `proxies`, the hooks are given
/// those proxy variables as the daemon's environment, and each prints
/// its environment too.
async fn what_hooks_printing_secrets_left(
    agent: impl Fn(domain::planning::HookEntry) -> domain::planning::Hooks,
    plan_hooks: impl Fn(domain::planning::HookEntry) -> domain::planning::Hooks,
    fails: bool,
    proxies: &[(&str, &str)],
) -> (Vec<HookOutcome>, String, Vec<String>) {
    use std::os::unix::fs::PermissionsExt as _;
    capture();
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let password = scratch.path().join("password");
    std::fs::write(&password, format!("{HOOK_PASSWORD}\n")).expect("the password is written");
    std::fs::set_permissions(&password, std::fs::Permissions::from_mode(0o600))
        .expect("the password file is the owner's");
    let says = scratch.path().join("says");
    let forms = [
        HOOK_PASSWORD.to_owned(),
        // As the url crate writes it in a URL's userinfo.
        "canary-hook%2Fpass%40word".to_owned(),
        HOOK_INLINE.to_owned(),
        // As Rust's `Debug` prints it between its quotes.
        "canary-inline\\\"key".to_owned(),
        HOOK_OTHER.to_owned(),
    ];
    std::fs::write(&says, forms.join("\n")).expect("what the hook prints is written");
    let hook = if proxies.is_empty() {
        format!("cat '{}'", says.display())
    } else {
        format!("sh -c 'cat \"$1\" && env' hook '{}'", says.display())
    };
    let hook = domain::planning::HookEntry::parse(&hook).expect("a hook");
    let alias = domain::RepositoryAlias::parse("demo").expect("a repository alias");
    let repository = |settings| domain::planning::RepositoryConfig {
        locations: Default::default(),
        id: None,
        settings,
        maintenance: domain::planning::Maintenance::None,
        group_by: Default::default(),
        hooks: Default::default(),
    };
    let mut home = domain::planning::Plan::new(
        domain::PlanId::parse("home").expect("a plan ID"),
        alias.clone(),
        vec![domain::planning::Source::parse("/home/demo").expect("a source")],
    );
    home.hooks = plan_hooks(hook.clone());
    let config = domain::planning::AgentConfig::new(
        domain::planning::AgentSettings::default(),
        agent(hook),
        BTreeMap::from([
            (
                alias,
                repository(RepositorySettings {
                    repository: "/srv/demo-repo".to_owned(),
                    repo_hot: None,
                    password: SecretRef::File {
                        path: password.display().to_string(),
                        read: FileRead::FirstLine,
                    },
                    options: [("secret_access_key".to_owned(), HOOK_INLINE.to_owned())].into(),
                    options_hot: Default::default(),
                    options_cold: Default::default(),
                }),
            ),
            (
                domain::RepositoryAlias::parse("other").expect("a repository alias"),
                repository(RepositorySettings {
                    repository: "/srv/other-repo".to_owned(),
                    repo_hot: None,
                    password: SecretRef::Inline(domain::secrets::SecretValue::new(HOOK_OTHER)),
                    options: Default::default(),
                    options_hot: Default::default(),
                    options_cold: Default::default(),
                }),
            ),
        ]),
        vec![home],
    )
    .expect("a valid configuration");
    let config_store = Arc::new(InMemoryConfigStore::new());
    app::ports::ConfigStore::save(&*config_store, &config)
        .await
        .expect("the config saves");
    let scripted = engine();
    if fails {
        *scripted
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Err(FailureReason::EngineFailed {
            detail: "demo failure".to_owned(),
        });
    }
    let hooks = if proxies.is_empty() {
        ProcessHookRunner::new()
    } else {
        let path = std::env::var_os("PATH").unwrap_or_default();
        ProcessHookRunner::new().with_environment(
            std::iter::once((std::ffi::OsString::from("PATH"), path)).chain(proxies.iter().map(
                |(name, value)| {
                    (
                        std::ffi::OsString::from(name),
                        std::ffi::OsString::from(value),
                    )
                },
            )),
        )
    };
    let kept = Arc::new(KeptHooks {
        runner: hooks,
        outcomes: Mutex::new(Vec::new()),
    });
    let (ends, heard) = app::retry::run_ends();
    let runner = PlanRunner {
        config: config_store,
        resolve: Arc::new(ResolveRepository::new(Arc::new(SystemSecrets::default()))),
        agent_id: AgentId::from_uuid(Uuid::from_u128(1)),
        engine: scripted,
        locations: None,
        retention: Arc::new(engine::RusticRetention),
        store: Arc::new(app::fakes::InMemoryOperationStore::new()),
        registry: Arc::new(CancelRegistry::new()),
        hooks: kept.clone(),
        events: Arc::new(BroadcastEventBus::new()),
        queue: Arc::new(RunQueue::new()),
        ends,
        runs: crate::composition::runs::Runs::default(),
        readings: Arc::new(app::fakes::InMemoryStatsStore::new()),
        full_disk_access: None,
        machine: Arc::default(),
        leases: None,
        running: Arc::default(),
        left_out: KeyFilesLeftOut::default(),
        writes: Arc::default(),
    };

    Operations::start_backup(&runner, "home".to_owned(), Trigger::Manual)
        .await
        .expect("the backup starts");
    tokio::time::timeout(std::time::Duration::from_secs(10), heard.next())
        .await
        .expect("the run ends");

    let page = Operations::history(&runner, None, None, None)
        .await
        .expect("the history reads");
    let row = serde_json::to_string(&page).expect("the history serializes");
    let log = captured_log::lines()
        .into_iter()
        .map(|(_, line)| line)
        .collect();
    let outcomes = kept
        .outcomes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    (outcomes, row, log)
}

/// Every hook that ran printed, and none of what it printed, the history
/// or the log holds a secret, in any of its forms (I4): the hook's own
/// words are kept with each secret masked.
fn assert_no_secret_left(outcomes: &[HookOutcome], row: &str, log: &[String], hooks: usize) {
    assert_eq!(outcomes.len(), hooks, "every hook ran: {outcomes:?}");
    for outcome in outcomes {
        assert!(outcome.succeeded(), "{outcome:?}");
        let text = &outcome.stdout.text;
        assert!(
            text.contains(domain::secrets::REDACTED),
            "the hook printed: {text}"
        );
        assert!(
            !text.contains("canary"),
            "a secret is in the hook's output: {text}"
        );
    }
    assert!(!row.contains("canary"), "a secret is in the history: {row}");
    for line in log {
        assert!(!line.contains("canary"), "a secret is in the log: {line}");
    }
}

/// A plan's hook that prints the run's secrets has them masked in the
/// output the run keeps (§10.2, I4, #853): the password the run read and
/// what the configuration holds as text, the run's repository's and
/// another's.
#[tokio::test]
async fn a_plans_hook_has_the_runs_secrets_masked_in_its_output() {
    let (outcomes, row, log) = what_hooks_printing_secrets_left(
        |_| domain::planning::Hooks::default(),
        |hook| domain::planning::Hooks {
            run_before: vec![hook.clone()],
            run_finally: vec![hook],
            ..Default::default()
        },
        false,
        &[],
    )
    .await;
    assert_no_secret_left(&outcomes, &row, &log, 2);
}

/// The agent's hooks run in the same run, after its secrets were read,
/// and have them masked as a plan's do.
#[tokio::test]
async fn the_agents_hooks_have_the_runs_secrets_masked_in_their_output() {
    let (outcomes, row, log) = what_hooks_printing_secrets_left(
        |hook| domain::planning::Hooks {
            run_before: vec![hook.clone()],
            run_finally: vec![hook],
            ..Default::default()
        },
        |_| domain::planning::Hooks::default(),
        false,
        &[],
    )
    .await;
    assert_no_secret_left(&outcomes, &row, &log, 2);
}

/// The hooks of a run that failed have its secrets masked too: the
/// secrets were read before the run started.
#[tokio::test]
async fn a_failed_runs_hooks_have_its_secrets_masked_in_their_output() {
    let (outcomes, row, log) = what_hooks_printing_secrets_left(
        |_| domain::planning::Hooks::default(),
        |hook| domain::planning::Hooks {
            run_failed: vec![hook.clone()],
            run_finally: vec![hook],
            ..Default::default()
        },
        true,
        &[],
    )
    .await;
    assert!(
        row.contains(r#""status":"failed""#),
        "the run failed: {row}"
    );
    assert_no_secret_left(&outcomes, &row, &log, 2);
}

/// A hook is given the daemon's proxy variables, credentials and all,
/// for a program it starts to use, and has those credentials masked in
/// what it prints, as a worker has (§10.2, I4): with a password, and as
/// a token in place of a user, in either case of the variable's name.
#[tokio::test]
async fn a_hook_has_the_proxy_credentials_of_its_environment_masked_in_its_output() {
    let (outcomes, row, log) = what_hooks_printing_secrets_left(
        |_| domain::planning::Hooks::default(),
        |hook| domain::planning::Hooks {
            run_before: vec![hook],
            ..Default::default()
        },
        false,
        &[
            (
                "HTTPS_PROXY",
                "http://demo:canary-proxy-pass@proxy.demo:3128",
            ),
            ("all_proxy", "socks5://canary-proxy-token@proxy.demo:1080"),
        ],
    )
    .await;
    assert_no_secret_left(&outcomes, &row, &log, 1);
}
