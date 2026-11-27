//! Steps for `features/maintenance/ownership.feature`: the hub's registry of
//! maintenance owners, driven through its use case with the in-memory
//! registries and a clock of the hub's own that moves only when a step
//! moves it (technical design §7.3, I1). A machine's claim is its report of
//! the one repository with the request `local`; a machine that stops using
//! a repository reports none. The hub's use case is made once, at the
//! hub's start; a restart makes it again over the same registry, and a
//! restore over a copy the registry's backup kept. A machine's own side,
//! whether it may maintain a repository, is its leases use case on an
//! in-memory cache and a clock of its own, which may run off the hub's and
//! moves with it; what it sees of its repositories is an engine of
//! in-memory repositories, listed through its daemon's listings and those
//! its sync runs before each session. A machine that pairs does so in
//! process, on that cache, with a hub's pairing use case of its own. The
//! hub signs each lease it grants with keys of its own, made when a grant
//! is first signed, and checks the floors each report carries against
//! them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use app::fakes::{
    FakeClock, FakeSecrets, FakeSigning, InMemoryAgentRegistry, InMemoryConfigStore,
    InMemoryIdentity, InMemoryLeaseCache, InMemoryOperationStore, InMemoryOwnershipRegistry,
    ListingEngine, RecordingNotifier, SeededRandom,
};
use app::leases::{MaintenanceLeases, WatchPauses};
use app::notifications::Notify;
use app::others_snapshots::{ListingsDue, NoteListings, OthersSnapshots};
use app::ownership::{Granting, Reported, RepositoryOwners};
use app::pairing::{AgentPairing, HubKeys, HubPairing, proven_floors, signed_grants};
use app::ports::{
    ClaimDecision, Clock, ConfigStore, Engine, EngineEvent, Identity, Job, OwnershipRegistry,
    RepositoryAccess, SnapshotSummary,
};
use app::secrets::ResolveRepository;
use app::snapshots::ListSnapshots;
use cucumber::{given, then, when};
use domain::execution::plan_tags::{MACHINE_TAG_PREFIX, PLAN_TAG_PREFIX};
use domain::fleet::{
    AgentRecord, AgentStatus, Fingerprint, GENERATION, Hostname, HubAddress, HubPin, PublicKey,
    SeedFile, TokenTls,
};
use domain::maintenance::{
    ClaimOutcome, ClaimRefused, EpochLower, Lease, LeaseEpoch, NoLease, ProvenFloor,
    ReleaseRefused, RepoGrants, RepositoryOwnership, RepositoryReport, RepositoryReports,
};
use domain::notifications::{
    Channel, ChannelKind, ChannelName, EventKind, Notifications, Rule, RuleName,
};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{
    AgentId, OperationId, RepositoryAlias, RepositoryId, SignedDuration, SnapshotId, Timestamp,
    Uuid,
};
use futures::StreamExt;

use super::Agent;

/// The hub's side: its clock, its registries, its use case since its last
/// start, and what each machine's last report came to.
pub(super) struct OwnershipWorld {
    clock: Arc<FakeClock>,
    agents: Arc<InMemoryAgentRegistry>,
    owners: Arc<InMemoryOwnershipRegistry>,
    hub: Arc<RepositoryOwners>,
    /// The registry as the last backup of the hub's database kept it.
    backup: Option<InMemoryOwnershipRegistry>,
    machines: BTreeMap<String, AgentId>,
    repositories: BTreeMap<String, RepositoryId>,
    /// What the hub decided of each machine's last report.
    last: BTreeMap<String, Reported>,
    /// The lease each machine was last granted.
    granted: BTreeMap<String, Lease>,
    /// The two machines that claimed at once.
    racers: Option<(String, String)>,
    released: Option<Result<(), ReleaseRefused>>,
    /// Each machine's own side: its leases and its clock.
    holders: BTreeMap<String, Holder>,
    /// Whether two machines were both let maintain one repository at once.
    both_maintained: bool,
    /// The hub a machine last paired with, when one did.
    paired_with: Option<PairingHub>,
    /// How many hubs machines have paired with, each with keys of its own.
    hubs_made: u64,
    /// The keys the hub signs its grants with, once it has signed one.
    keys: Option<HubKeys>,
    /// Each machine's configuration and notification rules, for a machine
    /// that watches its maintenance.
    watchers: BTreeMap<String, Watcher>,
}

/// A machine's configuration, the rules that read it, and what they sent.
struct Watcher {
    config: Arc<dyn ConfigStore>,
    notify: Arc<Notify>,
    notifier: Arc<RecordingNotifier>,
}

/// A hub a machine pairs with: its pairing use case and its keys.
struct PairingHub {
    pairing: HubPairing,
    keys: HubKeys,
}

/// The binding both sides of an in-process pairing agree on.
const EXPORTER: [u8; 32] = [0xee; 32];

/// A machine's own side: its key files, the leases it keeps and their
/// cache, its clock, the grants it refused last as below its floor, what
/// its listings showed of other machines' snapshots, the repositories it
/// asks to maintain, and what it last told the hub of them.
struct Holder {
    identity: Arc<InMemoryIdentity>,
    cache: Arc<InMemoryLeaseCache>,
    leases: Arc<MaintenanceLeases>,
    clock: Arc<FakeClock>,
    refused: Vec<EpochLower>,
    others: Arc<OthersSnapshots>,
    maintains: Vec<RepositoryId>,
    told: Vec<RepositoryReport>,
    /// The repositories as the machine sees them, and how often each was
    /// listed.
    engine: Arc<ListingEngine>,
}

impl Default for OwnershipWorld {
    fn default() -> Self {
        let clock = Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        ));
        let agents = Arc::new(InMemoryAgentRegistry::new());
        let owners = Arc::new(InMemoryOwnershipRegistry::new());
        Self {
            hub: Arc::new(RepositoryOwners::new(
                agents.clone(),
                owners.clone(),
                clock.clone(),
            )),
            clock,
            agents,
            owners,
            backup: None,
            machines: BTreeMap::new(),
            repositories: BTreeMap::new(),
            last: BTreeMap::new(),
            granted: BTreeMap::new(),
            racers: None,
            released: None,
            holders: BTreeMap::new(),
            both_maintained: false,
            paired_with: None,
            hubs_made: 0,
            keys: None,
            watchers: BTreeMap::new(),
        }
    }
}

impl OwnershipWorld {
    fn use_case(&self) -> Arc<RepositoryOwners> {
        self.hub.clone()
    }

    /// The hub starts again over `owners`.
    fn start_hub(&mut self, owners: Arc<InMemoryOwnershipRegistry>) {
        self.owners = owners;
        self.hub = Arc::new(RepositoryOwners::new(
            self.agents.clone(),
            self.owners.clone(),
            self.clock.clone(),
        ));
    }

    /// Registers `name` on the hub, approved or waiting for approval.
    fn register(&mut self, name: &str, approved: bool) {
        let n = u8::try_from(self.machines.len() + 1).unwrap();
        let agent_id = AgentId::from_uuid(Uuid::from_u128(u128::from(n)));
        let now = self.clock.now();
        self.agents.hold(AgentRecord {
            agent_id,
            key: PublicKey::from_bytes([n; 32]).unwrap(),
            hostname: Hostname::parse(name).unwrap(),
            status: if approved {
                AgentStatus::Approved { at: now }
            } else {
                AgentStatus::Pending { since: now }
            },
            counter: 1,
            last_seen: now,
            zone: None,
        });
        self.machines.insert(name.to_owned(), agent_id);
    }

    fn machine(&self, name: &str) -> AgentId {
        *self
            .machines
            .get(name)
            .expect("a machine the scenario named")
    }

    /// Names `name` as a machine the hub has no record of.
    fn unknown(&mut self, name: &str) {
        let agent_id = AgentId::from_uuid(Uuid::from_u128(u128::from(u8::MAX)));
        self.machines.insert(name.to_owned(), agent_id);
    }

    fn repository(&mut self, name: &str) -> RepositoryId {
        let next = self.repositories.len();
        self.repositories
            .entry(name.to_owned())
            .or_insert_with(|| {
                let digit = char::from_digit(u32::try_from(next + 10).unwrap(), 16).unwrap();
                RepositoryId::parse(&digit.to_string().repeat(64)).unwrap()
            })
            .clone()
    }

    /// `machine` reports `entries` as every repository it uses, each with
    /// the floor it holds on it and its proof.
    async fn report(&mut self, machine: &str, entries: Vec<RepositoryReport>) {
        let agent = self.machine(machine);
        let entries = match self.holders.get(machine) {
            Some(holder) => holder
                .leases
                .with_floors(entries)
                .await
                .expect("the cache answers"),
            None => entries,
        };
        let reports = RepositoryReports::new(entries);
        let proven = self.proven(agent, &reports);
        let reported = self
            .use_case()
            .report(agent, &reports, &proven)
            .await
            .expect("the registry answers");
        if let Reported::Decided(decided) = &reported {
            for decision in decided {
                if let ClaimOutcome::Granted(lease) = decision.outcome {
                    self.granted.insert(machine.to_owned(), lease);
                }
            }
        }
        self.last.insert(machine.to_owned(), reported);
    }

    async fn claim(&mut self, machine: &str, repository: &str, others_snapshots: bool) {
        let repository = self.repository(repository);
        self.report(
            machine,
            vec![RepositoryReport {
                repository,
                request: Maintenance::Local,
                holds_other_agents_snapshots: others_snapshots,
                floor: None,
            }],
        )
        .await;
    }

    /// `machine` reports `repository` with the request `none`.
    async fn report_without_claim(&mut self, machine: &str, repository: &str) {
        let repository = self.repository(repository);
        self.report(
            machine,
            vec![RepositoryReport {
                repository,
                request: Maintenance::None,
                holds_other_agents_snapshots: false,
                floor: None,
            }],
        )
        .await;
    }

    /// The one outcome of `machine`'s last report.
    fn outcome(&self, machine: &str) -> ClaimOutcome {
        match self.last.get(machine) {
            Some(Reported::Decided(decided)) => match decided.as_slice() {
                [ClaimDecision { outcome, .. }] => *outcome,
                other => panic!("{machine} reported one repository, decided {other:?}"),
            },
            other => panic!("{machine}'s report was decided: {other:?}"),
        }
    }

    async fn ownership(&mut self, repository: &str) -> RepositoryOwnership {
        let repository = self.repository(repository);
        self.owners
            .ownership(&repository)
            .await
            .expect("the registry answers")
    }

    async fn binding(&mut self, repository: &str) -> Option<Lease> {
        let now = self.clock.now();
        self.ownership(repository).await.binding(now).copied()
    }

    /// Moves the hub's clock and every machine's by `by`.
    fn pass(&self, by: SignedDuration) {
        self.clock.advance(by);
        for holder in self.holders.values() {
            holder.clock.advance(by);
        }
    }

    /// Starts `machine`'s own side, paired with the hub or not, its clock
    /// `skew` off the hub's.
    async fn start_machine(&mut self, machine: &str, paired: bool, skew: SignedDuration) {
        let identity = Arc::new(InMemoryIdentity::default());
        if paired {
            let key = |byte| PublicKey::from_bytes([byte; 32]).unwrap();
            let pin = HubPin::new(
                key(7),
                TokenTls::Ca,
                key(9),
                HubAddress::parse("https://hub.example:9898").unwrap(),
            );
            identity.write_pin(&pin).await.unwrap();
        }
        let clock = Arc::new(FakeClock::new(self.clock.now().checked_add(skew).unwrap()));
        let cache = Arc::new(InMemoryLeaseCache::default());
        let leases = Arc::new(MaintenanceLeases::new(
            identity.clone(),
            cache.clone(),
            clock.clone(),
        ));
        self.holders.insert(
            machine.to_owned(),
            Holder {
                identity,
                cache,
                leases,
                clock,
                refused: Vec::new(),
                others: Arc::default(),
                maintains: Vec::new(),
                told: Vec::new(),
                engine: Arc::default(),
            },
        );
    }

    /// `machine`'s own side, paired with the hub and on its time when no
    /// step started it otherwise.
    async fn holder(&mut self, machine: &str) -> &mut Holder {
        if !self.holders.contains_key(machine) {
            self.start_machine(machine, true, SignedDuration::ZERO)
                .await;
        }
        self.holders.get_mut(machine).unwrap()
    }

    /// `machine` takes `grants`, its answer to a request it sent now on its
    /// own clock.
    async fn take(&mut self, machine: &str, grants: &RepoGrants) {
        let holder = self.holder(machine).await;
        let asked_at = holder.clock.now();
        holder.refused = holder
            .leases
            .take(grants, 0, asked_at)
            .await
            .expect("the cache answers");
    }

    /// `machine` claims `repository` and takes the hub's grants in answer;
    /// grants the hub withholds leave it what it holds.
    async fn hear(&mut self, machine: &str, repository: &str) {
        let id = self.repository(repository);
        let agent = self.machine(machine);
        let asked_at = self.holder(machine).await.clock.now();
        let reports = self
            .holder(machine)
            .await
            .leases
            .with_floors(vec![RepositoryReport {
                repository: id,
                request: Maintenance::Local,
                holds_other_agents_snapshots: false,
                floor: None,
            }])
            .await
            .expect("the cache answers");
        let reports = RepositoryReports::new(reports);
        let proven = self.proven(agent, &reports);
        let granting = self
            .use_case()
            .grant(agent, &reports, &proven)
            .await
            .expect("the registry answers");
        let (generation, signed_at, leases) = match granting {
            Granting::Leases {
                generation,
                signed_at,
                leases,
            } => (generation, signed_at, leases),
            Granting::Withheld => return,
            Granting::NotApproved => panic!("an approved machine is answered"),
        };
        let keys = self.keys().await;
        let grants = signed_grants(&FakeSigning, keys, agent, generation, signed_at, leases)
            .expect("the hub signs");
        let holder = self.holder(machine).await;
        holder.refused = holder
            .leases
            .take(&grants, 0, asked_at)
            .await
            .expect("the cache answers");
    }

    /// The keys the hub signs its grants with.
    async fn keys(&mut self) -> &HubKeys {
        if self.keys.is_none() {
            self.keys = Some(self.make_hub().await.keys);
        }
        self.keys.as_ref().unwrap()
    }

    /// The floors in `agent`'s `reports` whose proof the hub's keys hold.
    fn proven(&self, agent: AgentId, reports: &RepositoryReports) -> Vec<ProvenFloor> {
        self.keys
            .as_ref()
            .map(|keys| proven_floors(&FakeSigning, keys, agent, reports))
            .unwrap_or_default()
    }

    /// Whether `machine` may maintain `repository` now.
    async fn permits(&mut self, machine: &str, repository: &str) -> Result<(), NoLease> {
        let id = self.repository(repository);
        self.holder(machine)
            .await
            .leases
            .permits(Some(&id))
            .await
            .map(|_| ())
    }

    /// Notes whether more than one machine may maintain `repository` now.
    async fn watch_both(&mut self, repository: &str) {
        let machines: Vec<String> = self.holders.keys().cloned().collect();
        let mut allowed = 0;
        for machine in machines {
            if self.permits(&machine, repository).await.is_ok() {
                allowed += 1;
            }
        }
        self.both_maintained |= allowed > 1;
    }

    /// The hub's grant to `machine` of `repository` at `number`, signed now
    /// for 7 days, as a hub rewound to any epoch would sign it: with the
    /// hub's keys, or with another hub's.
    async fn grant_at(
        &mut self,
        machine: &str,
        repository: &str,
        number: u64,
        another_hub: bool,
    ) -> RepoGrants {
        let agent = self.machine(machine);
        let repository = self.repository(repository);
        let signed_at = self.clock.now();
        let lease = Lease {
            holder: agent,
            epoch: epoch(number),
            expires_at: signed_at
                .checked_add(SignedDuration::from_hours(7 * 24))
                .unwrap(),
        };
        let other;
        let keys = if another_hub {
            other = self.make_hub().await.keys;
            &other
        } else {
            self.keys().await
        };
        signed_grants(
            &FakeSigning,
            keys,
            agent,
            GENERATION,
            signed_at,
            vec![(repository, lease)],
        )
        .expect("the hub signs")
    }

    /// A hub with keys of its own that no machine has paired with yet.
    async fn make_hub(&mut self) -> PairingHub {
        self.hubs_made += 1;
        let pairing = HubPairing::new(
            Arc::new(InMemoryAgentRegistry::new()),
            Arc::new(InMemoryIdentity::default()),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(self.hubs_made * 1_000)),
            self.clock.clone(),
            "0.5.0-hub".to_owned(),
        );
        let keys = pairing.keys().await.expect("the hub's keys");
        PairingHub { pairing, keys }
    }

    /// `machine` pairs with `hub` from a token the hub made, as the person
    /// runs `ladderback pair` on it.
    async fn pair(&mut self, machine: &str, hub: PairingHub) {
        let agent_id = self.machine(machine);
        let holder = self.holder(machine).await;
        let agent = AgentPairing::new(
            holder.identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(9_000)),
            holder.cache.clone(),
        );
        let token = hub
            .pairing
            .make_token(
                &hub.keys,
                HubAddress::parse("https://hub.example:9443").unwrap(),
                TokenTls::Pinned(Fingerprint::from_digest([7; 32])),
                None,
            )
            .await
            .expect("a token");
        let (connect, handshake) = agent
            .connect(token, agent_id, Hostname::parse(machine).unwrap(), None)
            .await
            .expect("the machine connects");
        let (challenge, hub_side) = hub
            .pairing
            .challenge(&hub.keys, &connect, Some(EXPORTER))
            .expect("the hub's challenge");
        let (pair, awaiting) = agent
            .answer_challenge(handshake, &challenge, Some(EXPORTER))
            .await
            .expect("the machine answers");
        let answer = hub
            .pairing
            .pair(&hub.keys, &hub_side, &pair)
            .await
            .expect("the hub's answer");
        agent
            .take_answer(awaiting, &answer)
            .await
            .expect("the machine is paired");
        self.paired_with = Some(hub);
    }

    /// `machine` lists `repository`, which holds one snapshot `agent`
    /// tagged, made `days` days ago on its clock, through the listings the
    /// machine's daemon runs.
    async fn list(&mut self, machine: &str, repository: &str, agent: AgentId, days: i64) {
        let id = self.repository(repository);
        let me = self.machine(machine);
        let holder = self.holder(machine).await;
        let made = holder
            .clock
            .now()
            .checked_sub(SignedDuration::from_hours(days * 24))
            .unwrap();
        holder
            .engine
            .holds(id.clone(), vec![snapshot_of(agent, made)]);
        let noting = NoteListings::new(
            holder.engine.clone(),
            Arc::clone(&holder.others),
            me,
            holder.clock.clone(),
        );
        let running = noting
            .run(
                OperationId::from_uuid(Uuid::from_u128(1)),
                Job::Ls(access(id)),
            )
            .await
            .expect("the listing runs");
        let _: Vec<EngineEvent> = running.events.collect().await;
    }

    /// `machine` syncs with the hub: its sync first lists each repository
    /// it asks to maintain whose listing doesn't stand, then reports them.
    async fn sync(&mut self, machine: &str) {
        let me = self.machine(machine);
        let names: BTreeMap<RepositoryId, String> = self
            .repositories
            .iter()
            .map(|(name, id)| (id.clone(), name.clone()))
            .collect();
        let holder = self.holder(machine).await;
        let config = Arc::new(InMemoryConfigStore::new());
        config
            .save(&maintaining(&holder.maintains, &names))
            .await
            .expect("the configuration is saved");
        let listing = ListSnapshots::new(
            Arc::new(NoteListings::new(
                holder.engine.clone(),
                Arc::clone(&holder.others),
                me,
                holder.clock.clone(),
            )),
            holder.clock.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::default())),
            Arc::default(),
        );
        ListingsDue::new(
            config,
            Arc::new(InMemoryOperationStore::new()),
            Arc::clone(&holder.others),
            listing,
            Arc::new(SeededRandom::new(1)),
            me,
            holder.clock.clone(),
        )
        .list()
        .await;
        self.report_uses(machine).await;
    }

    /// `machine` reports every repository it asks to maintain, as its sync
    /// with the hub does, at the time its clock says.
    async fn report_uses(&mut self, machine: &str) {
        let me = self.machine(machine);
        let holder = self.holder(machine).await;
        let now = holder.clock.now();
        let told: Vec<RepositoryReport> = holder
            .maintains
            .iter()
            .map(|id| {
                holder
                    .others
                    .report(id.clone(), Maintenance::Local, &me, now)
            })
            .collect();
        holder.told.clone_from(&told);
        self.report(machine, told).await;
    }

    /// What `machine` last told the hub of `repository`.
    async fn told(&mut self, machine: &str, repository: &str) -> RepositoryReport {
        let id = self.repository(repository);
        self.holder(machine)
            .await
            .told
            .iter()
            .find(|report| report.repository == id)
            .cloned()
            .expect("the machine reported the repository")
    }
}

fn epoch(number: u64) -> LeaseEpoch {
    LeaseEpoch::new(number).expect("an epoch above 0")
}

/// Another machine's agent.
fn another_machine() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0xa0))
}

/// The agent of a machine's earlier installation, before it was
/// reinstalled under a new machine ID.
fn earlier_installation() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0xe0))
}

/// A snapshot `agent`'s plan `home` made at `time`.
fn snapshot_of(agent: AgentId, time: Timestamp) -> SnapshotSummary {
    SnapshotSummary {
        id: SnapshotId::parse(&"5".repeat(64)).unwrap(),
        time,
        offset: domain::Offset::UTC,
        hostname: "laptop".to_owned(),
        label: String::new(),
        tags: vec![
            format!("{PLAN_TAG_PREFIX}home"),
            format!("{MACHINE_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
        ],
        paths: vec!["/home/demo".to_owned()],
        paths_truncated: false,
        tags_truncated: false,
    }
}

/// A configuration that asks to maintain each of `repositories` here, under
/// its name in `names`.
fn maintaining(
    repositories: &[RepositoryId],
    names: &BTreeMap<RepositoryId, String>,
) -> AgentConfig {
    let entries = repositories
        .iter()
        .map(|id| {
            let name = names.get(id).expect("a named repository");
            let entry = RepositoryConfig {
                locations: Default::default(),
                id: Some(id.clone()),
                settings: RepositorySettings {
                    repository: format!("/srv/{name}"),
                    repo_hot: None,
                    password: SecretRef::Inline(SecretValue::new("demo-password")),
                    options: BTreeMap::new(),
                    options_hot: BTreeMap::new(),
                    options_cold: BTreeMap::new(),
                },
                maintenance: Maintenance::Local,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            };
            (RepositoryAlias::parse(name).expect("an alias"), entry)
        })
        .collect();
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        entries,
        Vec::new(),
    )
    .expect("a valid configuration")
}

/// The access a listing of the repository `id` opens it with.
fn access(id: RepositoryId) -> RepositoryAccess {
    RepositoryAccess {
        repository: "/srv/photos".to_owned(),
        repo_hot: None,
        password: SecretValue::new("demo-password"),
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
        secrets: Vec::new(),
        id: Some(id),
        entry: None,
    }
}

#[given(expr = "a hub that approved the machines {string} and {string}")]
fn a_hub_that_approved(world: &mut Agent, first: String, second: String) {
    world.ownership.register(&first, true);
    world.ownership.register(&second, true);
}

#[given(expr = "the machine {string} is waiting for the hub's approval")]
fn waiting_for_approval(world: &mut Agent, name: String) {
    world.ownership.register(&name, false);
}

#[given(expr = "the hub approved the machine {string}")]
fn approved(world: &mut Agent, name: String) {
    world.ownership.register(&name, true);
}

#[given(expr = "the machine {string} isn't known to the hub")]
fn unknown_to_the_hub(world: &mut Agent, name: String) {
    world.ownership.unknown(&name);
}

#[given(expr = "{string} holds the lease on {string}")]
async fn holds_the_lease(world: &mut Agent, machine: String, repository: String) {
    world.ownership.claim(&machine, &repository, false).await;
    assert!(
        matches!(world.ownership.outcome(&machine), ClaimOutcome::Granted(_)),
        "{machine} was granted {repository}"
    );
}

#[given(expr = "{string} can't reach the hub any more")]
fn cant_reach_the_hub(_world: &mut Agent, _machine: String) {
    // A machine that can't reach its hub reports nothing: no step of the
    // scenario has it report again.
}

#[when(expr = "{string} claims the repository {string}")]
async fn claims(world: &mut Agent, machine: String, repository: String) {
    world.ownership.claim(&machine, &repository, false).await;
}

#[when(expr = "{string} claims the repository {string}, which holds another machine's snapshots")]
async fn claims_with_others_snapshots(world: &mut Agent, machine: String, repository: String) {
    world.ownership.claim(&machine, &repository, true).await;
}

#[when(expr = "{string} and {string} claim the repository {string} at the same moment")]
async fn claim_at_once(world: &mut Agent, first: String, second: String, repository: String) {
    let id = world.ownership.repository(&repository);
    let entry = |id: RepositoryId| {
        RepositoryReports::new([RepositoryReport {
            repository: id,
            request: Maintenance::Local,
            holds_other_agents_snapshots: false,
            floor: None,
        }])
    };
    let use_case = world.ownership.use_case();
    let (a, b) = (
        world.ownership.machine(&first),
        world.ownership.machine(&second),
    );
    let (first_report, second_report) = (entry(id.clone()), entry(id));
    let (one, other) = futures::join!(
        use_case.report(a, &first_report, &[]),
        use_case.report(b, &second_report, &[])
    );
    world
        .ownership
        .last
        .insert(first.clone(), one.expect("the registry answers"));
    world
        .ownership
        .last
        .insert(second.clone(), other.expect("the registry answers"));
    world.ownership.racers = Some((first, second));
}

#[when(expr = "{string} stops using {string}")]
async fn stops_using(world: &mut Agent, machine: String, _repository: String) {
    world.ownership.report(&machine, Vec::new()).await;
}

#[when(expr = "{string} releases its lease on {string} at epoch {int}")]
async fn releases(world: &mut Agent, machine: String, repository: String, number: u64) {
    let agent = world.ownership.machine(&machine);
    let repository = world.ownership.repository(&repository);
    let released = world
        .ownership
        .use_case()
        .release(agent, &repository, epoch(number))
        .await
        .expect("the registry answers");
    world.ownership.released = Some(released);
}

#[given(expr = "{int} days passed on the hub")]
#[when(expr = "{int} days pass on the hub")]
fn days_pass(world: &mut Agent, days: i64) {
    world.ownership.pass(SignedDuration::from_hours(24 * days));
}

#[when("1 hour passes on the hub")]
fn an_hour_passes(world: &mut Agent) {
    world.ownership.pass(SignedDuration::from_hours(1));
}

#[when(expr = "the hub's clock is set back {int} days")]
fn clock_set_back(world: &mut Agent, days: i64) {
    world
        .ownership
        .clock
        .set_back(SignedDuration::from_hours(24 * days));
}

#[when(expr = "the hub's clock is set forward {int} days")]
fn clock_set_forward(world: &mut Agent, days: i64) {
    // Only the wall clock steps: the hub's monotonic time doesn't move.
    world
        .ownership
        .clock
        .suspend(SignedDuration::from_hours(24 * days));
}

#[given(expr = "{string} reported the repository {string} without claiming it")]
async fn reported_without_claim(world: &mut Agent, machine: String, repository: String) {
    world
        .ownership
        .report_without_claim(&machine, &repository)
        .await;
}

#[given("the hub's database is backed up")]
fn database_backed_up(world: &mut Agent) {
    world.ownership.backup = Some(world.ownership.owners.backup());
}

#[when("the hub is restored from that backup and starts")]
fn restored_and_starts(world: &mut Agent) {
    let backup = world.ownership.backup.take().expect("a backup was made");
    world.ownership.start_hub(Arc::new(backup));
}

#[when("the hub restarts")]
fn hub_restarts(world: &mut Agent) {
    let owners = world.ownership.owners.clone();
    world.ownership.start_hub(owners);
}

#[when(expr = "7 days and 1 hour pass while {string} and {string} keep claiming {string}")]
async fn time_passes_while_claiming(
    world: &mut Agent,
    first: String,
    second: String,
    repository: String,
) {
    for _ in 0..7 {
        world.ownership.pass(SignedDuration::from_hours(24));
        world.ownership.claim(&first, &repository, false).await;
        world.ownership.claim(&second, &repository, false).await;
    }
    world.ownership.pass(SignedDuration::from_hours(1));
    world.ownership.claim(&first, &repository, false).await;
    world.ownership.claim(&second, &repository, false).await;
}

#[then(expr = "{string} holds the lease on {string} at epoch {int}, for 7 days from now")]
async fn holds_for_7_days(world: &mut Agent, machine: String, repository: String, number: u64) {
    let expected = Lease {
        holder: world.ownership.machine(&machine),
        epoch: epoch(number),
        expires_at: world
            .ownership
            .clock
            .now()
            .checked_add(SignedDuration::from_hours(7 * 24))
            .unwrap(),
    };
    assert_eq!(world.ownership.binding(&repository).await, Some(expected));
    assert!(
        matches!(
            world.ownership.outcome(&machine),
            ClaimOutcome::Granted(lease) | ClaimOutcome::Renewed(lease) if lease == expected
        ),
        "{machine} was told of its lease: {:?}",
        world.ownership.last.get(&machine)
    );
}

#[then(expr = "{string} holds the lease on {string} at epoch {int}")]
async fn holds_at_epoch(world: &mut Agent, machine: String, repository: String, number: u64) {
    let holder = world.ownership.machine(&machine);
    let lease = world.ownership.binding(&repository).await;
    assert_eq!(
        lease.map(|lease| (lease.holder, lease.epoch)),
        Some((holder, epoch(number)))
    );
}

#[then(expr = "exactly one of them holds the lease on {string}")]
async fn one_of_them_holds(world: &mut Agent, repository: String) {
    let (first, second) = world
        .ownership
        .racers
        .clone()
        .expect("two machines claimed");
    let holders = [&first, &second]
        .into_iter()
        .filter(|machine| matches!(world.ownership.outcome(machine), ClaimOutcome::Granted(_)))
        .count();
    assert_eq!(holders, 1, "{:?}", world.ownership.last);
    let lease = world.ownership.binding(&repository).await.expect("a lease");
    assert!(
        [first, second]
            .iter()
            .any(|machine| world.ownership.machine(machine) == lease.holder)
    );
}

#[then("the other's claim was refused because that one holds the lease")]
fn the_other_refused(world: &mut Agent) {
    let (first, second) = world
        .ownership
        .racers
        .clone()
        .expect("two machines claimed");
    let (winner, loser) = if matches!(world.ownership.outcome(&first), ClaimOutcome::Granted(_)) {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(
        world.ownership.outcome(&loser),
        ClaimOutcome::Refused(ClaimRefused::HeldByAnother(
            world.ownership.machine(&winner)
        ))
    );
}

#[then(expr = "the claim of {string} was refused because {string} holds the lease")]
fn refused_held(world: &mut Agent, machine: String, holder: String) {
    assert_eq!(
        world.ownership.outcome(&machine),
        ClaimOutcome::Refused(ClaimRefused::HeldByAnother(
            world.ownership.machine(&holder)
        ))
    );
}

#[then(expr = "the claim of {string} was refused because another machine uses the repository")]
fn refused_shared(world: &mut Agent, machine: String) {
    assert_eq!(
        world.ownership.outcome(&machine),
        ClaimOutcome::Refused(ClaimRefused::Shared)
    );
}

#[then(expr = "the claim of {string} was refused because the hub is in recovery mode")]
fn refused_recovering(world: &mut Agent, machine: String) {
    assert_eq!(
        world.ownership.outcome(&machine),
        ClaimOutcome::Refused(ClaimRefused::Recovering)
    );
}

#[then("the hub is in recovery mode")]
async fn in_recovery(world: &mut Agent) {
    assert!(world.ownership.use_case().recovering().await.unwrap());
}

#[then("the hub isn't in recovery mode")]
async fn not_in_recovery(world: &mut Agent) {
    assert!(!world.ownership.use_case().recovering().await.unwrap());
}

#[when("a person confirms on the hub that the old hub is stopped or gone")]
async fn confirms_old_hub_gone(world: &mut Agent) {
    assert!(world.ownership.use_case().end_recovery().await.unwrap());
}

#[then(expr = "the claim of {string} was refused because the hub has just started")]
fn refused_starting(world: &mut Agent, machine: String) {
    assert_eq!(
        world.ownership.outcome(&machine),
        ClaimOutcome::Refused(ClaimRefused::Starting)
    );
}

#[then(
    expr = "the lease of {string} on {string} wasn't renewed, because another machine uses the repository"
)]
async fn not_renewed(world: &mut Agent, machine: String, repository: String) {
    let granted = *world
        .ownership
        .granted
        .get(&machine)
        .expect("a granted lease");
    assert_eq!(
        world.ownership.outcome(&machine),
        ClaimOutcome::Unrenewed(granted)
    );
    assert_eq!(world.ownership.binding(&repository).await, Some(granted));
}

#[then(expr = "the lease of {string} on {string} still runs out 7 days after it was granted")]
async fn still_runs_out(world: &mut Agent, machine: String, repository: String) {
    let granted = *world
        .ownership
        .granted
        .get(&machine)
        .expect("a granted lease");
    assert_eq!(
        world.ownership.outcome(&machine),
        ClaimOutcome::Renewed(granted)
    );
    assert_eq!(world.ownership.binding(&repository).await, Some(granted));
}

#[then(expr = "nobody holds the lease on {string}")]
async fn nobody_holds(world: &mut Agent, repository: String) {
    assert_eq!(world.ownership.binding(&repository).await, None);
}

#[then("the release was refused")]
fn release_refused(world: &mut Agent) {
    assert_eq!(world.ownership.released, Some(Err(ReleaseRefused)));
}

#[then(expr = "the hub decided nothing of the report of {string}")]
fn decided_nothing(world: &mut Agent, machine: String) {
    assert_eq!(
        world.ownership.last.get(&machine),
        Some(&Reported::NotApproved)
    );
}

#[given(expr = "{string} is paired with the hub and has heard no grant from it")]
async fn paired_no_grant(world: &mut Agent, machine: String) {
    world
        .ownership
        .start_machine(&machine, true, SignedDuration::ZERO)
        .await;
}

#[given(expr = "{string} isn't paired with a hub")]
async fn not_paired(world: &mut Agent, machine: String) {
    world
        .ownership
        .start_machine(&machine, false, SignedDuration::ZERO)
        .await;
}

#[given(expr = "the clock of {string} runs {int} days behind the hub's")]
async fn clock_behind(world: &mut Agent, machine: String, days: i64) {
    world
        .ownership
        .start_machine(&machine, true, SignedDuration::from_hours(-24 * days))
        .await;
}

#[given(expr = "the clock of {string} runs {int} days ahead of the hub's")]
async fn clock_ahead(world: &mut Agent, machine: String, days: i64) {
    world
        .ownership
        .start_machine(&machine, true, SignedDuration::from_hours(24 * days))
        .await;
}

#[given(expr = "{string} claims the repository {string} and hears the hub's grants")]
#[when(expr = "{string} claims the repository {string} and hears the hub's grants")]
async fn claims_and_hears(world: &mut Agent, machine: String, repository: String) {
    world.ownership.hear(&machine, &repository).await;
}

#[when(
    expr = "{string} claims the repository {string} and hears the hub's grants every hour for {int} days"
)]
async fn hears_every_hour(world: &mut Agent, machine: String, repository: String, days: u32) {
    for _ in 0..days * 24 {
        world.ownership.hear(&machine, &repository).await;
        world.ownership.watch_both(&repository).await;
        world.ownership.pass(SignedDuration::from_hours(1));
        world.ownership.watch_both(&repository).await;
    }
    world.ownership.hear(&machine, &repository).await;
    world.ownership.watch_both(&repository).await;
}

#[given(expr = "{string} heard a grant of {string} at epoch {int}")]
async fn heard_a_grant(world: &mut Agent, machine: String, repository: String, number: u64) {
    let grants = world
        .ownership
        .grant_at(&machine, &repository, number, false)
        .await;
    world.ownership.take(&machine, &grants).await;
    assert_eq!(world.ownership.holder(&machine).await.refused, vec![]);
}

#[given(expr = "{string} heard a grant of {string} at epoch {int} signed by another hub's key")]
async fn heard_another_hubs_grant(
    world: &mut Agent,
    machine: String,
    repository: String,
    number: u64,
) {
    // The hub has keys of its own, which signed none of it.
    world.ownership.keys().await;
    let grants = world
        .ownership
        .grant_at(&machine, &repository, number, true)
        .await;
    world.ownership.take(&machine, &grants).await;
    assert_eq!(world.ownership.holder(&machine).await.refused, vec![]);
}

#[when(expr = "the hub grants {string} the repository {string} at epoch {int}")]
async fn hub_grants_at(world: &mut Agent, machine: String, repository: String, number: u64) {
    let grants = world
        .ownership
        .grant_at(&machine, &repository, number, false)
        .await;
    world.ownership.take(&machine, &grants).await;
}

#[given(expr = "{string} lost its hub and holds no pin")]
async fn lost_its_hub(world: &mut Agent, machine: String) {
    let holder = world.ownership.holder(&machine).await;
    let fresh = Arc::new(InMemoryIdentity::default());
    // The machine keeps its own key: only the hub's pin is gone.
    if let Some(seed) = holder.identity.seed(SeedFile::AgentKey).await.unwrap() {
        fresh.keep_seed(SeedFile::AgentKey, seed).await.unwrap();
    }
    holder.identity = fresh.clone();
    holder.leases = Arc::new(MaintenanceLeases::new(
        fresh,
        holder.cache.clone(),
        holder.clock.clone(),
    ));
}

#[given(expr = "{string} paired with a hub from its token")]
async fn paired_from_a_token(world: &mut Agent, machine: String) {
    world
        .ownership
        .start_machine(&machine, false, SignedDuration::ZERO)
        .await;
    let hub = world.ownership.make_hub().await;
    world.ownership.pair(&machine, hub).await;
}

#[when(expr = "{string} pairs with a new hub")]
async fn pairs_with_a_new_hub(world: &mut Agent, machine: String) {
    let hub = world.ownership.make_hub().await;
    world.ownership.pair(&machine, hub).await;
}

#[when(expr = "{string} pairs again with that hub from a new token")]
async fn pairs_again(world: &mut Agent, machine: String) {
    let hub = world
        .ownership
        .paired_with
        .take()
        .expect("a hub the machine paired with");
    world.ownership.pair(&machine, hub).await;
}

#[when(expr = "the new hub grants {string} the repository {string} at epoch {int}")]
async fn new_hub_grants_at(world: &mut Agent, machine: String, repository: String, number: u64) {
    hub_grants_at(world, machine, repository, number).await;
}

#[when(expr = "{string} stops using {string} and the hub acknowledges its release")]
async fn stops_and_released(world: &mut Agent, machine: String, repository: String) {
    let agent = world.ownership.machine(&machine);
    let id = world.ownership.repository(&repository);
    let leases = world.ownership.holder(&machine).await.leases.clone();
    for (held, at) in leases.held().await.expect("the cache answers") {
        if held != id {
            continue;
        }
        world
            .ownership
            .use_case()
            .release(agent, &held, at)
            .await
            .expect("the registry answers")
            .expect("the hub takes the holder's release");
        leases.released(&held, at).await.expect("the cache answers");
    }
    world.ownership.report(&machine, Vec::new()).await;
}

#[then(expr = "{string} may maintain {string}")]
async fn may_maintain(world: &mut Agent, machine: String, repository: String) {
    assert_eq!(world.ownership.permits(&machine, &repository).await, Ok(()));
}

#[then(expr = "{string} may not maintain {string}, because it holds no lease on it")]
async fn may_not_none_held(world: &mut Agent, machine: String, repository: String) {
    assert_eq!(
        world.ownership.permits(&machine, &repository).await,
        Err(NoLease::NoneHeld)
    );
}

#[then(expr = "{string} may not maintain {string}, because its lease ran out")]
async fn may_not_ran_out(world: &mut Agent, machine: String, repository: String) {
    assert_eq!(
        world.ownership.permits(&machine, &repository).await,
        Err(NoLease::RanOut)
    );
}

#[then(expr = "{string} and {string} never both may maintain {string}")]
async fn never_both(world: &mut Agent, first: String, second: String, repository: String) {
    let both = world.ownership.permits(&first, &repository).await.is_ok()
        && world.ownership.permits(&second, &repository).await.is_ok();
    assert!(
        !world.ownership.both_maintained && !both,
        "{first} and {second} both maintained {repository} at once"
    );
}

#[then(expr = "{string} refused the grant of {string} as below the highest it has seen")]
async fn refused_lower(world: &mut Agent, machine: String, repository: String) {
    let id = world.ownership.repository(&repository);
    let refused = &world.ownership.holder(&machine).await.refused;
    assert!(
        matches!(refused.as_slice(), [EpochLower { repository, .. }] if *repository == id),
        "{refused:?}"
    );
}

/// `machine`'s configuration: `repository`, of its ID on the hub, asking
/// `maintenance` of it, and, when `rule` names a channel, a rule that sends
/// `repo.maintenance_paused` to that webhook channel.
async fn asks_to_maintain_with(
    world: &mut Agent,
    machine: &str,
    repository: &str,
    maintenance: Maintenance,
    rule: Option<&str>,
) {
    let id = world.ownership.repository(repository);
    let entry = RepositoryConfig {
        locations: Default::default(),
        id: Some(id),
        settings: RepositorySettings {
            repository: format!("/srv/{repository}"),
            repo_hot: None,
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    let mut config = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(RepositoryAlias::parse(repository).unwrap(), entry)]),
        vec![],
    )
    .unwrap();
    let secrets = Arc::new(FakeSecrets::new());
    if let Some(name) = rule {
        secrets.set_env("DEMO_OPS_URL", "https://ops.example/hook");
        let channel = ChannelName::parse(name).unwrap();
        let notifications = Notifications::new(
            BTreeMap::from([(
                channel.clone(),
                Channel::new(
                    ChannelKind::Webhook,
                    SecretRef::Env("DEMO_OPS_URL".to_owned()),
                    None,
                )
                .unwrap(),
            )]),
            BTreeMap::from([(
                RuleName::parse(name).unwrap(),
                Rule::new(
                    BTreeSet::from([EventKind::RepoMaintenancePaused]),
                    channel,
                    BTreeSet::new(),
                    None,
                )
                .unwrap(),
            )]),
        )
        .unwrap();
        config = config.with_notifications(notifications).unwrap();
    }
    let store = InMemoryConfigStore::new();
    store.save(&config).await.unwrap();
    let config: Arc<dyn ConfigStore> = Arc::new(store);
    let notifier = Arc::new(RecordingNotifier::new());
    let notify = Arc::new(Notify::new(
        config.clone(),
        Arc::new(InMemoryOperationStore::new()),
        secrets,
        notifier.clone(),
    ));
    world.ownership.watchers.insert(
        machine.to_owned(),
        Watcher {
            config,
            notify,
            notifier,
        },
    );
}

#[given(expr = "{string} asks to maintain {string}")]
async fn asks_to_maintain(world: &mut Agent, machine: String, repository: String) {
    asks_to_maintain_with(world, &machine, &repository, Maintenance::Local, None).await;
}

#[given(expr = "{string} keeps {string} without maintaining it")]
async fn keeps_without_maintaining(world: &mut Agent, machine: String, repository: String) {
    asks_to_maintain_with(world, &machine, &repository, Maintenance::None, None).await;
}

/// `machine` turns maintenance of `repository` on, as the Snapshots page's
/// toggle does, which notes a pause at once when it holds no lease (#1671).
#[when(expr = "{string} turns on maintenance of {string}")]
async fn turns_on_maintenance(world: &mut Agent, machine: String, repository: String) {
    let leases = world.ownership.holder(&machine).await.leases.clone();
    let config = watcher(world, &machine).config.clone();
    let changed = app::repositories::ChangeMaintainer::new(
        config,
        Arc::new(InMemoryOperationStore::new()),
        Arc::default(),
    )
    .with_leases(leases)
    .change(&RepositoryAlias::parse(&repository).unwrap(), true)
    .await;
    assert_eq!(changed, Ok(Maintenance::Local));
}

#[given(
    expr = "{string} asks to maintain {string}, with a rule that sends \"repo.maintenance_paused\" to the webhook channel {string}"
)]
async fn asks_to_maintain_with_a_rule(
    world: &mut Agent,
    machine: String,
    repository: String,
    channel: String,
) {
    asks_to_maintain_with(
        world,
        &machine,
        &repository,
        Maintenance::Local,
        Some(&channel),
    )
    .await;
}

#[when("a day passes on the hub")]
fn a_day_passes(world: &mut Agent) {
    world.ownership.pass(SignedDuration::from_hours(24));
}

#[when(expr = "{string} watches its maintenance")]
async fn watches(world: &mut Agent, machine: String) {
    let leases = world.ownership.holder(&machine).await.leases.clone();
    let watcher = watcher(world, &machine);
    let watch = WatchPauses::new(watcher.config.clone(), leases, watcher.notify.clone());
    assert_eq!(watch.watch().await, vec![]);
}

/// Since when `machine` says its maintenance of `repository` is paused.
async fn paused(world: &mut Agent, machine: &str, repository: &str) -> Option<Timestamp> {
    let id = world.ownership.repository(repository);
    world
        .ownership
        .holder(machine)
        .await
        .leases
        .paused(Some(&id))
        .await
}

/// `days` days before now on `machine`'s clock.
async fn days_ago(world: &mut Agent, machine: &str, days: i64) -> Timestamp {
    let now = world.ownership.holder(machine).await.clock.now();
    now.checked_sub(SignedDuration::from_hours(24 * days))
        .unwrap()
}

#[then(
    expr = "maintenance of {string} on {string} is paused since its lease ran out {int} days ago"
)]
#[then(expr = "maintenance of {string} on {string} is paused since {int} days ago")]
async fn paused_since(world: &mut Agent, repository: String, machine: String, days: i64) {
    let since = days_ago(world, &machine, days).await;
    assert_eq!(paused(world, &machine, &repository).await, Some(since));
}

/// Paused since `machine`'s clock reads now: turning maintenance on noted
/// the pause the moment it saw no lease, with no watch since (#1671).
#[then(expr = "maintenance of {string} on {string} is paused since now")]
async fn paused_since_now(world: &mut Agent, repository: String, machine: String) {
    let now = world.ownership.holder(&machine).await.clock.now();
    assert_eq!(paused(world, &machine, &repository).await, Some(now));
}

#[then(expr = "maintenance of {string} on {string} isn't paused")]
async fn not_paused(world: &mut Agent, repository: String, machine: String) {
    assert_eq!(paused(world, &machine, &repository).await, None);
}

/// The watcher of the machine the scenario named.
fn watcher<'a>(world: &'a Agent, machine: &str) -> &'a Watcher {
    world
        .ownership
        .watchers
        .get(machine)
        .expect("a machine the scenario named")
}

#[then(expr = "{string} sent no notification")]
fn sent_nothing(world: &mut Agent, machine: String) {
    let sent = watcher(world, &machine).notifier.delivered();
    assert!(sent.is_empty(), "{sent:?}");
}

#[then(
    expr = "{string} was sent {string} for the repository {string}, paused since {int} days ago"
)]
async fn was_sent(
    world: &mut Agent,
    channel: String,
    event: String,
    repository: String,
    days: i64,
) {
    let machine = world.ownership.watchers.keys().next().cloned().unwrap();
    let since = days_ago(world, &machine, days).await;
    let sent = watcher(world, &machine).notifier.delivered();
    let [notification] = sent.as_slice() else {
        panic!("one notification to {channel}: {sent:?}");
    };
    assert_eq!(notification.event.kind.name(), event);
    assert_eq!(
        notification.event.repository,
        Some(RepositoryAlias::parse(&repository).unwrap())
    );
    assert_eq!(notification.event.at, since);
    assert_eq!(notification.event.operation, None);
}

#[then(expr = "{string} was sent {string} only once")]
fn sent_once(world: &mut Agent, channel: String, event: String) {
    let sent: Vec<_> = world
        .ownership
        .watchers
        .values()
        .flat_map(|watcher| watcher.notifier.delivered())
        .filter(|notification| notification.event.kind.name() == event)
        .collect();
    assert_eq!(sent.len(), 1, "{event} to {channel}: {sent:?}");
}

#[given(expr = "{string} asks to maintain the repository {string}")]
async fn asks_to_maintain_the_repository(world: &mut Agent, machine: String, repository: String) {
    let id = world.ownership.repository(&repository);
    world.ownership.holder(&machine).await.maintains.push(id);
}

#[given(
    expr = "{string} listed {string}, which holds a snapshot another machine made {int} days ago"
)]
async fn listed_another_machines(
    world: &mut Agent,
    machine: String,
    repository: String,
    days: i64,
) {
    world
        .ownership
        .list(&machine, &repository, another_machine(), days)
        .await;
}

#[given(expr = "{string} listed {string}, which holds only its own snapshots")]
async fn listed_its_own(world: &mut Agent, machine: String, repository: String) {
    let me = world.ownership.machine(&machine);
    world.ownership.list(&machine, &repository, me, 1).await;
}

#[given(
    expr = "{string} listed {string}, which holds the history its earlier installation made {int} days ago"
)]
async fn listed_earlier_history(world: &mut Agent, machine: String, repository: String, days: i64) {
    world
        .ownership
        .list(&machine, &repository, earlier_installation(), days)
        .await;
}

/// An adoption rewrites the history under this machine's agent tag (§16,
/// decision 62), so the listing after it shows this machine's tag.
#[given(
    expr = "{string} listed {string}, which holds the history its earlier installation made {int} days ago, adopted since"
)]
async fn listed_adopted_history(world: &mut Agent, machine: String, repository: String, days: i64) {
    let me = world.ownership.machine(&machine);
    world.ownership.list(&machine, &repository, me, days).await;
}

#[when(expr = "{string} reports the repositories it uses")]
async fn reports_its_repositories(world: &mut Agent, machine: String) {
    world.ownership.report_uses(&machine).await;
}

#[then(expr = "{string} told the hub that {string} holds another machine's snapshots")]
async fn told_others(world: &mut Agent, machine: String, repository: String) {
    let told = world.ownership.told(&machine, &repository).await;
    assert!(told.holds_other_agents_snapshots, "{told:?}");
}

#[then(expr = "{string} told the hub that {string} holds no other machine's snapshots")]
async fn told_no_others(world: &mut Agent, machine: String, repository: String) {
    let told = world.ownership.told(&machine, &repository).await;
    assert!(!told.holds_other_agents_snapshots, "{told:?}");
}

#[given(expr = "{string} holds only snapshots {string} made")]
async fn holds_only_own(world: &mut Agent, repository: String, machine: String) {
    let id = world.ownership.repository(&repository);
    let me = world.ownership.machine(&machine);
    let holder = world.ownership.holder(&machine).await;
    let made = holder.clock.now() - SignedDuration::from_hours(24);
    holder.engine.holds(id, vec![snapshot_of(me, made)]);
}

#[given(expr = "{string} synced with the hub")]
#[when(expr = "{string} syncs with the hub")]
async fn syncs(world: &mut Agent, machine: String) {
    world.ownership.sync(&machine).await;
}

#[when(expr = "another machine backs up to {string}")]
async fn another_backs_up(world: &mut Agent, repository: String) {
    let id = world.ownership.repository(&repository);
    for holder in world.ownership.holders.values() {
        let made = holder.clock.now();
        holder
            .engine
            .holds(id.clone(), vec![snapshot_of(another_machine(), made)]);
    }
}

#[then(expr = "{string} listed {string} {int} time(s)")]
async fn listed_times(world: &mut Agent, machine: String, repository: String, times: usize) {
    let id = world.ownership.repository(&repository);
    let holder = world.ownership.holder(&machine).await;
    assert_eq!(holder.engine.listings(&id), times);
}
