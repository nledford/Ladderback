//! Steps for `features/maintenance/ownership.feature`: the hub's registry of
//! maintenance owners, driven through its use case with the in-memory
//! registries and a clock of the hub's own that moves only when a step
//! moves it (technical design §7.3, I1). A machine's claim is its report of
//! the one repository with the request `local`; a machine that stops using
//! a repository reports none.

use std::collections::BTreeMap;
use std::sync::Arc;

use app::fakes::{FakeClock, InMemoryAgentRegistry, InMemoryOwnershipRegistry};
use app::ownership::{Reported, RepositoryOwners};
use app::ports::{ClaimDecision, Clock, OwnershipRegistry};
use cucumber::{given, then, when};
use domain::fleet::{AgentRecord, AgentStatus, Hostname, PublicKey};
use domain::maintenance::{
    ClaimOutcome, ClaimRefused, Lease, LeaseEpoch, ReleaseRefused, RepositoryOwnership,
    RepositoryReport, RepositoryReports,
};
use domain::planning::Maintenance;
use domain::{AgentId, RepositoryId, SignedDuration, Timestamp, Uuid};

use super::Agent;

/// The hub's side: its clock, its registries, and what each machine's last
/// report came to.
pub(super) struct OwnershipWorld {
    clock: Arc<FakeClock>,
    agents: Arc<InMemoryAgentRegistry>,
    owners: Arc<InMemoryOwnershipRegistry>,
    machines: BTreeMap<String, AgentId>,
    repositories: BTreeMap<String, RepositoryId>,
    /// What the hub decided of each machine's last report.
    last: BTreeMap<String, Reported>,
    /// The lease each machine was last granted.
    granted: BTreeMap<String, Lease>,
    /// The two machines that claimed at once.
    racers: Option<(String, String)>,
    released: Option<Result<(), ReleaseRefused>>,
}

impl Default for OwnershipWorld {
    fn default() -> Self {
        Self {
            clock: Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).unwrap(),
            )),
            agents: Arc::new(InMemoryAgentRegistry::new()),
            owners: Arc::new(InMemoryOwnershipRegistry::new()),
            machines: BTreeMap::new(),
            repositories: BTreeMap::new(),
            last: BTreeMap::new(),
            granted: BTreeMap::new(),
            racers: None,
            released: None,
        }
    }
}

impl OwnershipWorld {
    fn use_case(&self) -> RepositoryOwners {
        RepositoryOwners::new(self.agents.clone(), self.owners.clone(), self.clock.clone())
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

    /// `machine` reports `entries` as every repository it uses.
    async fn report(&mut self, machine: &str, entries: Vec<RepositoryReport>) {
        let agent = self.machine(machine);
        let reported = self
            .use_case()
            .report(agent, &RepositoryReports::new(entries))
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

    fn pass(&self, by: SignedDuration) {
        self.clock.advance(by);
    }
}

fn epoch(number: u64) -> LeaseEpoch {
    LeaseEpoch::new(number).expect("an epoch above 0")
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
        }])
    };
    let use_case = world.ownership.use_case();
    let (a, b) = (
        world.ownership.machine(&first),
        world.ownership.machine(&second),
    );
    let (first_report, second_report) = (entry(id.clone()), entry(id));
    let (one, other) = futures::join!(
        use_case.report(a, &first_report),
        use_case.report(b, &second_report)
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
