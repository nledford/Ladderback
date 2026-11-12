//! Steps for `features/execution/announcement.feature`, for the fake-backed
//! acceptance runner: a page that reads the history whenever it hears an
//! operation started or ended, as a client that refetches on the news does,
//! and a log that can't take an operation's end.

use std::sync::{Arc, Mutex, PoisonError};

use api::{ErrorCode, UiEvent};
use app::execution::RunOperation;
use app::fakes::InMemoryOperationStore;
use app::maintenance::RunMaintenance;
use app::ports::{
    EngineResult, EventBus, HistoryPage, HistoryQuery, OperationDetail, OperationRecord,
    OperationStore, Start, StoreError,
};
use async_trait::async_trait;
use cucumber::{given, then};
use domain::execution::{Operation, OperationStatus};
use domain::{OperationId, Uuid};
use futures::future;
use futures::stream::{self, BoxStream, StreamExt};

use super::Agent;

/// What a page heard, and what the history held for the operation at the
/// moment it did.
#[derive(Clone, Debug)]
pub struct Heard {
    pub news: UiEvent,
    pub read: Option<OperationDetail>,
}

/// A page that, on hearing an operation started or ended, reads the history
/// for it: the client the news has to be safe for.
struct Page {
    history: Arc<dyn OperationStore>,
    heard: Mutex<Vec<Heard>>,
}

#[async_trait]
impl EventBus for Page {
    async fn publish(&self, news: UiEvent) {
        let told = match &news {
            UiEvent::OperationStarted { operation, .. }
            | UiEvent::OperationSucceeded { operation, .. }
            | UiEvent::OperationFailed { operation, .. } => Uuid::parse_str(operation).ok(),
            _ => None,
        };
        let read = match told {
            Some(uuid) => self
                .history
                .get(OperationId::from_uuid(uuid))
                .await
                .ok()
                .flatten(),
            None => None,
        };
        self.heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Heard { news, read });
    }

    async fn subscribe(&self) -> BoxStream<'static, UiEvent> {
        stream::empty().boxed()
    }
}

/// How the log treats the write of an operation's end.
#[derive(Clone, Copy, Debug)]
enum Fault {
    Refuses,
    NeverAnswers,
}

/// An operation log that takes every write but the end's.
struct FaultyLog {
    inner: Arc<InMemoryOperationStore>,
    fault: Fault,
}

#[async_trait]
impl OperationStore for FaultyLog {
    async fn recover(
        &self,
        reason: &domain::execution::FailureReason,
    ) -> Result<app::ports::Recovered, StoreError> {
        self.inner.recover(reason).await
    }

    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        if operation.status().is_finished() {
            match self.fault {
                Fault::Refuses => {
                    return Err(StoreError::Unavailable {
                        detail: "disk I/O error".to_owned(),
                    });
                }
                Fault::NeverAnswers => future::pending::<()>().await,
            }
        }
        self.inner.save(operation, result).await
    }

    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError> {
        self.inner.try_start(operation).await
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        self.inner.get(id).await
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        self.inner.list(query).await
    }

    async fn ever_succeeded(&self, plan: &domain::PlanId) -> Result<bool, StoreError> {
        self.inner.ever_succeeded(plan).await
    }
}

/// The page a scenario listens with, and the fault it gives the log.
#[derive(Default)]
pub struct AnnouncementWorld {
    page: Option<Arc<Page>>,
    fault: Option<Fault>,
}

impl AnnouncementWorld {
    /// `use_case`, announcing to the page when the scenario has one.
    pub fn listening(&self, use_case: RunOperation) -> RunOperation {
        match &self.page {
            Some(page) => use_case.with_events(page.clone()),
            None => use_case,
        }
    }

    /// `use_case`, announcing to the page when the scenario has one.
    pub fn listening_maintenance(&self, use_case: RunMaintenance) -> RunMaintenance {
        match &self.page {
            Some(page) => use_case.with_events(page.clone()),
            None => use_case,
        }
    }

    /// The log a run writes to: `store`, or one that can't take the end.
    pub fn log(&self, store: &Arc<InMemoryOperationStore>) -> Arc<dyn OperationStore> {
        match self.fault {
            Some(fault) => Arc::new(FaultyLog {
                inner: store.clone(),
                fault,
            }),
            None => store.clone(),
        }
    }

    fn heard(&self) -> Vec<Heard> {
        self.page
            .as_ref()
            .expect("the scenario has a page")
            .heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[given("a page that reads the history whenever it hears an operation started or ended")]
fn a_page(agent: &mut Agent) {
    agent.announcement.page = Some(Arc::new(Page {
        history: agent.store.clone(),
        heard: Mutex::default(),
    }));
}

#[given("the log refuses the write of the operation's end")]
fn log_refuses(agent: &mut Agent) {
    agent.announcement.fault = Some(Fault::Refuses);
}

#[given("the log never answers the write of the operation's end")]
fn log_never_answers(agent: &mut Agent) {
    agent.announcement.fault = Some(Fault::NeverAnswers);
}

/// The one end the page heard.
fn the_news(agent: &Agent) -> Heard {
    let mut ends: Vec<Heard> = agent
        .announcement
        .heard()
        .into_iter()
        .filter(|heard| {
            matches!(
                heard.news,
                UiEvent::OperationSucceeded { .. } | UiEvent::OperationFailed { .. }
            )
        })
        .collect();
    assert_eq!(ends.len(), 1, "one end: {ends:?}");
    ends.remove(0)
}

/// The one start the page heard.
fn the_start(agent: &Agent) -> Heard {
    let mut starts: Vec<Heard> = agent
        .announcement
        .heard()
        .into_iter()
        .filter(|heard| matches!(heard.news, UiEvent::OperationStarted { .. }))
        .collect();
    assert_eq!(starts.len(), 1, "one start: {starts:?}");
    starts.remove(0)
}

#[then("the page hears the operation started")]
fn hears_start(agent: &mut Agent) {
    let operation = agent.operation();
    let UiEvent::OperationStarted {
        operation: heard,
        started_at,
        ..
    } = the_start(agent).news
    else {
        panic!("the page heard something other than a start");
    };
    assert_eq!(heard, operation.id().as_uuid().hyphenated().to_string());
    assert_eq!(
        started_at,
        operation
            .started_at()
            .expect("the operation started")
            .to_string()
    );
}

#[then("the history the page read on hearing it started holds the operation as running")]
fn page_read_the_start(agent: &mut Agent) {
    assert_eq!(
        the_start(agent)
            .read
            .expect("the history held the operation when the page read it")
            .record
            .status,
        OperationStatus::Running
    );
}

#[then("the page hears the operation succeeded")]
fn hears_success(agent: &mut Agent) {
    let operation = agent.operation();
    assert_eq!(
        the_news(agent).news,
        UiEvent::OperationSucceeded {
            operation: operation.id().as_uuid().hyphenated().to_string(),
            finished_at: operation
                .finished_at()
                .expect("the operation ended")
                .to_string(),
        }
    );
}

fn code_of(word: &str) -> ErrorCode {
    match word {
        "operation-failed" => ErrorCode::OperationFailed,
        "engine-exited" => ErrorCode::EngineExited,
        "engine-unavailable" => ErrorCode::EngineUnavailable,
        other => panic!("unknown error code {other}"),
    }
}

#[then(expr = "the page hears the operation failed with the code {string}")]
fn hears_failure(agent: &mut Agent, code: String) {
    let operation = agent.operation();
    let UiEvent::OperationFailed {
        operation: heard,
        code: heard_code,
        finished_at,
        ..
    } = the_news(agent).news
    else {
        panic!("the page heard something other than a failure");
    };
    assert_eq!(heard, operation.id().as_uuid().hyphenated().to_string());
    assert_eq!(heard_code, code_of(&code));
    assert_eq!(
        finished_at,
        operation
            .finished_at()
            .expect("the operation ended")
            .to_string()
    );
}

#[then("the page has heard nothing")]
fn heard_nothing(agent: &mut Agent) {
    assert!(agent.announcement.heard().is_empty());
}

/// What the history held when the page read it.
fn what_the_page_read(agent: &Agent) -> OperationDetail {
    the_news(agent)
        .read
        .expect("the history held the operation when the page read it")
}

#[then("the history the page read holds the operation as it ended")]
fn page_read_the_end(agent: &mut Agent) {
    let read = what_the_page_read(agent);
    let ended = agent.finished();
    assert_eq!(
        read.record,
        OperationRecord::of(&ended.operation, ended.result.as_ref())
    );
    assert_eq!(read.warnings, ended.operation.warnings());
}

#[then("the history the page read holds the operation as running")]
fn page_read_it_running(agent: &mut Agent) {
    assert_eq!(
        what_the_page_read(agent).record.status,
        OperationStatus::Running
    );
}

#[then("the run reports a write the log did not take")]
fn run_reports_the_write(agent: &mut Agent) {
    assert!(
        !agent.finished().recording_errors.is_empty(),
        "the log took every write"
    );
}
