//! The hub's own routes under `/api/v1/hub` (§16, decisions 120 and 121):
//! the key its page shows, the pairing tokens the page makes and cancels,
//! the agents it lists, approves, rejects and removes, and its recovery
//! mode, which the page reads and a person ends (decision 137). A daemon
//! not serving as a hub answers them as it does any unknown path.
//!
//! Making a token, approving an agent and ending recovery mode are
//! high-impact actions: a session's only (no API token permission opens
//! them, so a token is refused before it gets here), confirmed by a fresh
//! step-up each time, and refused from beyond loopback while sign-in is off
//! (decision 120, "Who may make a token, approve or re-key"). Rejecting or
//! removing an agent only takes authority away: a session's only too, and
//! refused beyond loopback while sign-in is off, but with no step-up
//! (decision 154).

use std::net::IpAddr;
use std::sync::Arc;

use api::ErrorCode;
use app::hub_commands::CommandAnswer;
use app::pairing::{
    ApprovalAsk, CommandAsk, CommandAskRefused, HubPairing, HubPairingError, Machine, MadeToken,
    RecoveryAsk, RemovalAsk, TokenAsk, TokenAskRefused, progress_dto, time_of,
};
use axum::Router;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use domain::access::SessionTag;
use domain::access::step_up::StepUpConfirmation;
use domain::fleet::{
    AgentRecord, AgentStatus, ApprovalRefused, Fingerprint, HubAddress, HubCommand, HubRoute,
    HubScheme, ListedAgent, PlansReport, PublicKey, RemovalRefused, ReportedOutcome, RouteRefused,
    TokenId, TokenTls,
};
use domain::{AgentId, OperationId, PlanId, SignedDuration, Timestamp, Uuid};

use crate::AppState;

/// The path of one pairing token the page cancels: `DELETE`.
const PAIRING_TOKEN_PATH: &str = "/api/v1/hub/pairing-tokens/{id}";

/// The path the page approves one waiting machine at: `POST`, as
/// [`api::machine_approval_path`] names it.
const MACHINE_APPROVAL_PATH: &str = "/api/v1/hub/machines/{id}/approval";

/// The path the page rejects or removes one machine at: `POST`, as
/// [`api::machine_removal_path`] names it.
const MACHINE_REMOVAL_PATH: &str = "/api/v1/hub/machines/{id}/removal";

/// The path the page runs a plan on one machine at: `POST`, as
/// [`api::machine_runs_path`] names it.
const MACHINE_RUNS_PATH: &str = "/api/v1/hub/machines/{id}/runs";

/// The path the page cancels one machine's backup at: `POST`, as
/// [`api::machine_cancel_path`] names it.
const MACHINE_CANCEL_PATH: &str = "/api/v1/hub/machines/{id}/operations/{operation}/cancel";

/// `routes` with the hub's own.
pub(crate) fn routes(routes: Router<AppState>) -> Router<AppState> {
    routes
        .route(api::HUB_PATH, axum::routing::get(hub))
        .route(api::PAIRING_TOKENS_PATH, axum::routing::post(make_token))
        .route(PAIRING_TOKEN_PATH, axum::routing::delete(cancel_token))
        .route(api::HUB_MACHINES_PATH, axum::routing::get(agents))
        .route(api::HUB_DASHBOARD_PATH, axum::routing::get(machines))
        .route(MACHINE_APPROVAL_PATH, axum::routing::post(approve))
        .route(MACHINE_REMOVAL_PATH, axum::routing::post(remove))
        .route(MACHINE_RUNS_PATH, axum::routing::post(run_plan))
        .route(MACHINE_CANCEL_PATH, axum::routing::post(cancel_operation))
        .route(api::HUB_RECOVERY_PATH, axum::routing::get(recovery))
        .route(
            api::HUB_RECOVERY_CONFIRMATION_PATH,
            axum::routing::post(end_recovery),
        )
}

/// The hub's pairing, or none when this daemon doesn't serve as a hub.
fn pairing_of(state: &AppState) -> Option<Arc<HubPairing>> {
    state.sync.pairing().cloned()
}

/// The answer a daemon not serving as a hub gives the hub's routes: the
/// one an unknown path gets.
fn not_a_hub() -> Response {
    crate::refused(StatusCode::NOT_FOUND, ErrorCode::RouteUnknown)
}

/// The hub key's fingerprint, and the certificate a token is
/// offered first: the hub's own when the page came over its built-in TLS,
/// a certificate authority's when it came in plain, as through a proxy.
async fn hub(State(state): State<AppState>, request: Request) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let own = state.sync.certificate();
    let over_tls = request.extensions().get::<crate::BuiltInTls>().is_some();
    let certificate = if over_tls && own.is_some() {
        api::TokenCertificateDto::Pinned
    } else {
        api::TokenCertificateDto::Ca
    };
    match pairing.hub_key().await {
        Ok(key) => axum::Json(api::HubDto {
            fingerprint: Fingerprint::of_key(&key).to_string(),
            certificate,
            own_certificate: own.map(|fingerprint| fingerprint.to_string()),
        })
        .into_response(),
        Err(error) => hub_refusal(&error),
    }
}

/// A pairing token for a [`api::MakePairingTokenDto`], answered 201 with a
/// [`api::PairingTokenDto`].
async fn make_token(State(state): State<AppState>, request: Request) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let peer = match peer_of(&request) {
        Ok(peer) => peer,
        Err(refused) => return *refused,
    };
    let beyond = beyond_loopback(request.headers(), peer);
    // The body limit has already read the body, bounded by the API's limit
    // (`body_limit`), so this only takes the bytes it holds.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(api::MakePairingTokenDto {
        address,
        certificate,
        life_minutes,
        confirmation,
    }) = serde_json::from_slice(&body)
    else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    // A pinned token names the certificate the hub serves: one that
    // serves none has nothing to pin, so a pinned ask is no ask it reads.
    let tls = match certificate {
        api::TokenCertificateDto::Ca => TokenTls::Ca,
        api::TokenCertificateDto::Pinned => match state.sync.certificate() {
            Some(fingerprint) => TokenTls::Pinned(fingerprint),
            None => return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody),
        },
    };
    // A pinned certificate is served over TLS: an `http://` address has
    // none to pin.
    let Ok(address) = HubAddress::parse(&address) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::HubAddressInvalid);
    };
    if matches!(tls, TokenTls::Pinned(_))
        && HubRoute::of(&address, tls) == Err(RouteRefused::Malformed)
    {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::HubAddressInvalid);
    }
    let ask = TokenAsk {
        address,
        tls,
        life: life_minutes.map(|minutes| SignedDuration::from_mins(i64::from(minutes))),
        peer,
        beyond_loopback: beyond,
        // Checked after the sign-in rule: beyond loopback with sign-in off,
        // no confirmation helps.
        confirmation: StepUpConfirmation::parse(confirmation).ok(),
    };
    match pairing.make_token_asked(ask).await {
        Ok(made) => (StatusCode::CREATED, axum::Json(token_dto(&made))).into_response(),
        Err(refused) => ask_refusal(refused),
    }
}

/// The connection's address: a wrong password counts in its waits, as a
/// sign-in's does. Without it, refused as a sign-in is.
fn peer_of(request: &Request) -> Result<IpAddr, Box<Response>> {
    match request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .copied()
    {
        Some(ConnectInfo(crate::PeerAddr(peer))) => Ok(peer.ip()),
        None => {
            log::error!(
                "a step-up was asked for without its connection's address; the server must be served with its connect info"
            );
            Err(Box::new(crate::unavailable(
                ErrorCode::SignInUnavailable,
                Vec::new(),
                None,
            )))
        }
    }
}

/// The answer to an ask the hub refused before doing it.
fn ask_refusal(refused: TokenAskRefused) -> Response {
    match refused {
        TokenAskRefused::NeedsSignIn => {
            crate::refused(StatusCode::FORBIDDEN, ErrorCode::PairingNeedsSignIn)
        }
        TokenAskRefused::StepUpFailed => {
            crate::refused(StatusCode::FORBIDDEN, ErrorCode::StepUpFailed)
        }
        TokenAskRefused::StepUpWait { seconds } => crate::sign_in::wait_refused(seconds),
        TokenAskRefused::StepUpUnavailable(detail) => {
            // The platform's words name files of the data folder: the log
            // keeps them for whoever runs the daemon, not for whoever asked.
            log::warn!("the admin password could not be checked: {detail}");
            crate::unavailable(ErrorCode::SignInUnavailable, Vec::new(), None)
        }
        TokenAskRefused::Hub(error) => hub_refusal(&error),
    }
}

/// Every agent the hub has registered, oldest first, as a
/// [`api::HubAgentsDto`]: the page's Machines tab lists them.
async fn agents(State(state): State<AppState>) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    match pairing.agents_listed().await {
        Ok(listed) => axum::Json(api::HubAgentsDto {
            machines: listed
                .agents
                .iter()
                .map(|agent| agent_dto(agent, listed.at))
                .collect(),
        })
        .into_response(),
        Err(error) => hub_refusal(&error),
    }
}

/// Every approved machine, oldest first, with its latest operations, as a
/// [`api::HubMachinesDto`]: the page's dashboard shows them.
async fn machines(State(state): State<AppState>) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    match pairing.machines().await {
        Ok(machines) => axum::Json(api::HubMachinesDto {
            machines: machines.iter().map(machine_dto).collect(),
        })
        .into_response(),
        Err(error) => hub_refusal(&error),
    }
}

fn machine_dto(machine: &Machine) -> api::HubMachineDto {
    let record = &machine.record;
    api::HubMachineDto {
        machine_id: record.agent_id.as_uuid().to_string(),
        hostname: record.hostname.as_str().to_owned(),
        last_seen: record.last_seen.to_string(),
        online: machine.online,
        zone: record.zone.as_ref().map(|zone| zone.as_str().to_owned()),
        reports_dropped: machine.reports_dropped,
        operations: machine
            .operations
            .iter()
            .map(|operation| {
                let (outcome, code) = match &operation.outcome {
                    ReportedOutcome::Running => (api::MachineOutcomeDto::Running, None),
                    ReportedOutcome::Succeeded => (api::MachineOutcomeDto::Succeeded, None),
                    ReportedOutcome::Failed { code } => (
                        api::MachineOutcomeDto::Failed,
                        Some(code.as_str().to_owned()),
                    ),
                    ReportedOutcome::Canceled => (api::MachineOutcomeDto::Canceled, None),
                };
                api::MachineOperationDto {
                    operation: operation.op_id.as_uuid().to_string(),
                    started_at: operation.started_at.and_then(time_of),
                    ended_at: operation.ended_at.and_then(time_of),
                    outcome,
                    code,
                }
            })
            .collect(),
        plans: machine.plans.as_ref().map(plans_dto),
        progress: machine.progress.as_ref().map(progress_dto),
        accepts: machine
            .accepts
            .kinds()
            .map(|kind| match kind {
                domain::fleet::CommandKind::RunPlan => api::MachineCommandDto::RunPlan,
                domain::fleet::CommandKind::CancelOp => api::MachineCommandDto::CancelOp,
            })
            .collect(),
    }
}

/// A machine's plans as the dashboard shows them.
fn plans_dto(plans: &PlansReport) -> api::MachinePlansDto {
    api::MachinePlansDto {
        plans: plans
            .plans()
            .iter()
            .map(|plan| api::MachinePlanDto {
                plan: plan.plan.as_str().to_owned(),
                last_run: plan.last.and_then(|last| {
                    time_of(last.at).map(|at| api::MachinePlanRunDto {
                        at,
                        status: last.outcome.wire_name().to_owned(),
                    })
                }),
                next_run: plan.next_run.and_then(time_of),
            })
            .collect(),
        left_out: plans.left_out(),
    }
}

/// The waiting agent the path names approved, under the key the
/// [`api::ApproveAgentDto`] holds: the one the person compared. Answered
/// with the agent as a [`api::HubAgentDto`].
async fn approve(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let peer = match peer_of(&request) {
        Ok(peer) => peer,
        Err(refused) => return *refused,
    };
    let beyond = beyond_loopback(request.headers(), peer);
    // No agent ID was ever made from a path that isn't one.
    let Ok(agent_id) = Uuid::parse_str(&id).map(AgentId::from_uuid) else {
        return crate::refused(StatusCode::NOT_FOUND, ErrorCode::MachineUnknown);
    };
    // Bounded by the API's body limit, as a token's ask is.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(api::ApproveAgentDto { key, confirmation }) = serde_json::from_slice(&body) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(agent_key) = PublicKey::from_hex(&key) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let ask = ApprovalAsk {
        agent_id,
        agent_key,
        peer,
        beyond_loopback: beyond,
        confirmation: StepUpConfirmation::parse(confirmation).ok(),
    };
    match pairing.approve_asked(ask).await {
        Ok(Ok(record)) => {
            let listed = ListedAgent {
                record,
                looks_like: None,
                second_machine_at: None,
            };
            let at = approved_at(&listed.record);
            axum::Json(agent_dto(&listed, at)).into_response()
        }
        Ok(Err(ApprovalRefused::Unknown)) => {
            crate::refused(StatusCode::NOT_FOUND, ErrorCode::MachineUnknown)
        }
        Ok(Err(ApprovalRefused::AlreadyApproved)) => {
            crate::refused(StatusCode::CONFLICT, ErrorCode::MachineApprovedAlready)
        }
        Err(refused) => ask_refusal(refused),
    }
}

/// Whether the hub is in recovery mode, as a [`api::HubRecoveryDto`]: the
/// Machines tab shows its notice while it is (decision 137).
async fn recovery(State(state): State<AppState>) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    match pairing.recovering().await {
        Ok(recovering) => axum::Json(api::HubRecoveryDto { recovering }).into_response(),
        Err(error) => hub_refusal(&error),
    }
}

/// Recovery mode ended, once the step-up the [`api::EndRecoveryDto`] holds
/// confirms a person's word that the old hub is stopped or gone: answered
/// `204` whether or not the hub was in recovery mode (decision 137).
async fn end_recovery(State(state): State<AppState>, request: Request) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let peer = match peer_of(&request) {
        Ok(peer) => peer,
        Err(refused) => return *refused,
    };
    let beyond = beyond_loopback(request.headers(), peer);
    // Bounded by the API's body limit, as a token's ask is.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(api::EndRecoveryDto { confirmation }) = serde_json::from_slice(&body) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let ask = RecoveryAsk {
        peer,
        beyond_loopback: beyond,
        confirmation: StepUpConfirmation::parse(confirmation).ok(),
    };
    match pairing.end_recovery_asked(ask).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(refused) => ask_refusal(refused),
    }
}

/// The agent the path names rejected while it waits, or removed once
/// approved, under the key the [`api::RemoveAgentDto`] holds: the one the
/// person saw. Answered `204 No Content`; no step-up is asked.
async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let peer = match peer_of(&request) {
        Ok(peer) => peer,
        Err(refused) => return *refused,
    };
    let beyond = beyond_loopback(request.headers(), peer);
    // No agent ID was ever made from a path that isn't one.
    let Ok(agent_id) = Uuid::parse_str(&id).map(AgentId::from_uuid) else {
        return crate::refused(StatusCode::NOT_FOUND, ErrorCode::MachineUnknown);
    };
    // Bounded by the API's body limit, as an approval's is.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(api::RemoveAgentDto { key }) = serde_json::from_slice(&body) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(agent_key) = PublicKey::from_hex(&key) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let ask = RemovalAsk {
        agent_id,
        agent_key,
        beyond_loopback: beyond,
    };
    match pairing.remove_asked(ask).await {
        Ok(Ok(_)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(RemovalRefused::Unknown)) => {
            crate::refused(StatusCode::NOT_FOUND, ErrorCode::MachineUnknown)
        }
        Err(refused) => ask_refusal(refused),
    }
}

/// The run of the plan the [`api::MachineRunDto`] names on the machine the
/// path names (decision 176): `202 Accepted` with the backup that runs it,
/// new or already running, or the refusal of the hub or of the machine.
async fn run_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    session: Option<axum::Extension<crate::sign_in::SessionOf>>,
    request: Request,
) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let peer = match peer_of(&request) {
        Ok(peer) => peer,
        Err(refused) => return *refused,
    };
    let beyond = beyond_loopback(request.headers(), peer);
    let Ok(agent) = Uuid::parse_str(&id).map(AgentId::from_uuid) else {
        return crate::refused(StatusCode::NOT_FOUND, ErrorCode::MachineUnknown);
    };
    // Bounded by the API's body limit, as an approval's is.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(api::MachineRunDto { plan }) = serde_json::from_slice(&body) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(plan) = PlanId::parse(&plan) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let ask = CommandAsk {
        agent,
        command: HubCommand::RunPlan { plan: plan.clone() },
        asked_by: asked_by(session.as_ref()),
        beyond_loopback: beyond,
    };
    let subject = Subject {
        plan: Some(plan.as_str()),
        operation: None,
    };
    match pairing.command_asked(ask).await {
        Ok(CommandAnswer::Started(operation)) => {
            (StatusCode::ACCEPTED, command_done(operation)).into_response()
        }
        Ok(answer) => machine_answer_refused(&answer, &subject),
        Err(refused) => command_refused(refused),
    }
}

/// The cancel of the backup the path names on the machine it names
/// (decision 176): `200` with the canceled backup, or the refusal of the
/// hub or of the machine.
async fn cancel_operation(
    State(state): State<AppState>,
    Path((id, operation)): Path<(String, String)>,
    session: Option<axum::Extension<crate::sign_in::SessionOf>>,
    request: Request,
) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let peer = match peer_of(&request) {
        Ok(peer) => peer,
        Err(refused) => return *refused,
    };
    let beyond = beyond_loopback(request.headers(), peer);
    let Ok(agent) = Uuid::parse_str(&id).map(AgentId::from_uuid) else {
        return crate::refused(StatusCode::NOT_FOUND, ErrorCode::MachineUnknown);
    };
    // No operation ID was ever made from a path that isn't one; the
    // path's text is bounded by the request line's.
    let Ok(op) = Uuid::parse_str(&operation).map(OperationId::from_uuid) else {
        return crate::refused_with(
            StatusCode::NOT_FOUND,
            ErrorCode::OperationUnknown,
            vec![("operation", api::MsgArg::Text(operation))],
        );
    };
    let ask = CommandAsk {
        agent,
        command: HubCommand::CancelOp { op },
        asked_by: asked_by(session.as_ref()),
        beyond_loopback: beyond,
    };
    let subject = Subject {
        plan: None,
        operation: Some(&operation),
    };
    match pairing.command_asked(ask).await {
        Ok(CommandAnswer::Canceled(operation)) => {
            (StatusCode::OK, command_done(operation)).into_response()
        }
        Ok(answer) => machine_answer_refused(&answer, &subject),
        Err(refused) => command_refused(refused),
    }
}

/// The session that asked, as the machine hears of it: its tag, never its
/// token (I4). None with sign-in off.
fn asked_by(session: Option<&axum::Extension<crate::sign_in::SessionOf>>) -> Option<SessionTag> {
    session.map(|session| SessionTag::of(&session.token.digest()))
}

/// The backup `operation` a command ran or canceled, as the page reads it.
fn command_done(operation: OperationId) -> axum::Json<api::MachineCommandDoneDto> {
    axum::Json(api::MachineCommandDoneDto {
        operation: operation.as_uuid().hyphenated().to_string(),
    })
}

/// What a command was about, for the arguments of its refusal.
struct Subject<'a> {
    plan: Option<&'a str>,
    operation: Option<&'a str>,
}

/// The machine's `answer` that didn't do what was asked: its refusal, by
/// the code it gave, or `machine-run-refused` for an answer to another
/// command.
fn machine_answer_refused(answer: &CommandAnswer, subject: &Subject<'_>) -> Response {
    let (code, retry_at) = match answer {
        CommandAnswer::Refused { code, retry_at } => (*code, *retry_at),
        CommandAnswer::Started(_) | CommandAnswer::Canceled(_) => {
            log::warn!("a machine answered the hub's command with another's result");
            (ErrorCode::MachineRunRefused, None)
        }
    };
    let status = match code {
        ErrorCode::PlanUnknown | ErrorCode::OperationUnknown => StatusCode::NOT_FOUND,
        ErrorCode::CommandRateLimited => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::CONFLICT,
    };
    let args = code
        .args()
        .iter()
        .filter_map(|spec| {
            let arg = match spec.name {
                "plan" => api::MsgArg::Text(subject.plan?.to_owned()),
                "operation" => api::MsgArg::Text(subject.operation?.to_owned()),
                "retry-at" => time_arg(retry_at?),
                _ => return None,
            };
            Some((spec.name, arg))
        })
        .collect();
    crate::refused_with(status, code, args)
}

/// The hub's refusal of a command, by its kind.
fn command_refused(refused: CommandAskRefused) -> Response {
    let code = refused.code();
    match refused {
        CommandAskRefused::NeedsSignIn => crate::refused(StatusCode::FORBIDDEN, code),
        CommandAskRefused::MachineUnknown => crate::refused(StatusCode::NOT_FOUND, code),
        CommandAskRefused::MachineOfflineNextRun { .. } => todo!(),
        CommandAskRefused::MachineOffline
        | CommandAskRefused::CommandsUnsupported
        | CommandAskRefused::CommandsOff => crate::refused(StatusCode::CONFLICT, code),
        CommandAskRefused::Busy { retry_at } => crate::refused_with(
            StatusCode::TOO_MANY_REQUESTS,
            code,
            vec![("retry-at", time_arg(retry_at))],
        ),
        CommandAskRefused::NoAnswer => crate::refused(StatusCode::GATEWAY_TIMEOUT, code),
        CommandAskRefused::Unavailable => crate::unavailable(code, Vec::new(), None),
    }
}

/// `at` as a refusal's time argument.
fn time_arg(at: Timestamp) -> api::MsgArg {
    api::MsgArg::Timestamp {
        utc_ms: at.as_millisecond(),
        zone: None,
    }
}

/// When the agent's status began: approved just now, for one the hub
/// approved this moment.
fn approved_at(record: &AgentRecord) -> Timestamp {
    match record.status {
        AgentStatus::Pending { since } => since,
        AgentStatus::Approved { at } => at,
    }
}

/// The agent as the page lists it, `now` the time the list was read.
fn agent_dto(listed: &ListedAgent, now: Timestamp) -> api::HubAgentDto {
    let record = &listed.record;
    let (status, since) = match record.status {
        AgentStatus::Pending { since } => (api::AgentStatusDto::Pending, since),
        AgentStatus::Approved { at } => (api::AgentStatusDto::Approved, at),
    };
    // A clock set back shows no wait rather than a negative one.
    let seconds_ago = u64::try_from(since.duration_until(now).as_secs()).unwrap_or(0);
    api::HubAgentDto {
        machine_id: record.agent_id.as_uuid().to_string(),
        hostname: record.hostname.as_str().to_owned(),
        key: record.key.to_hex(),
        fingerprint: Fingerprint::of_key(&record.key).to_string(),
        status,
        since: since.to_string(),
        seconds_ago,
        looks_like: listed.looks_like.as_ref().map(|like| api::LookalikeDto {
            machine_id: like.agent_id.as_uuid().to_string(),
            hostname: like.hostname.as_str().to_owned(),
            last_seen: like.last_seen.to_string(),
        }),
        second_machine_at: listed.second_machine_at.map(|at| at.to_string()),
    }
}

/// The token as the page shows it, once. Its life left is the hub's own
/// count, which the page counts down on its monotonic clock, so a browser
/// whose clock is wrong still shows it right. A plain address shows no
/// certificate, so its token names none (#1343).
fn token_dto(made: &MadeToken) -> api::PairingTokenDto {
    api::PairingTokenDto {
        token: made.token.to_text().expose().to_owned(),
        token_id: made.id.to_string(),
        fingerprint: Fingerprint::of_key(&made.hub_key).to_string(),
        address: made.token.address().as_str().to_owned(),
        expires_at: made.expires_at.to_string(),
        seconds_left: u64::try_from(made.left.as_secs()).unwrap_or(0),
        certificate: match (made.token.address().scheme(), made.token.tls()) {
            (HubScheme::Http, _) => None,
            (HubScheme::Https, TokenTls::Pinned(_)) => Some(api::TokenCertificateDto::Pinned),
            (HubScheme::Https, TokenTls::Ca) => Some(api::TokenCertificateDto::Ca),
        },
    }
}

/// Cancels the unused token `id`: 204, or 404 `pairing-token-unknown` when
/// none is waiting by that ID.
async fn cancel_token(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let unknown = || crate::refused(StatusCode::NOT_FOUND, ErrorCode::PairingTokenUnknown);
    let Some(id) = TokenId::parse(&id) else {
        return unknown();
    };
    match pairing.revoke_token(id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => unknown(),
        Err(error) => hub_refusal(&error),
    }
}

/// What the hub couldn't do, by its kind; what it couldn't read or keep is
/// told in the daemon's log, not to whoever asked.
fn hub_refusal(error: &HubPairingError) -> Response {
    match error {
        HubPairingError::TokenLife => {
            crate::refused(StatusCode::BAD_REQUEST, ErrorCode::PairingTokenLife)
        }
        HubPairingError::TooManyUnusedTokens => {
            crate::refused(StatusCode::CONFLICT, ErrorCode::PairingTokenLimit)
        }
        other => {
            log::error!("the hub could not make or cancel a pairing token: {other:?}");
            crate::unavailable(ErrorCode::HubUnavailable, Vec::new(), None)
        }
    }
}

/// Whether a request came from beyond loopback: its `Host` isn't a loopback
/// name, or its connection's peer isn't a loopback address. A proxy on
/// this machine forwarding a remote browser shows a loopback peer, so the
/// `Host` it forwards counts too.
fn beyond_loopback(headers: &HeaderMap, peer: std::net::IpAddr) -> bool {
    !(peer.is_loopback() && crate::request_checks::host_is_loopback(headers))
}

#[cfg(test)]
mod tests;
