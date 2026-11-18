//! The protocol as a client sees it: one session over an in-memory pipe, a
//! stand-in [`Backend`] behind it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use api::{
    BuildDto, ErrorCode, ErrorDto, ExposureDto, NewProposalDto, OperationWarningDto, PlansDto,
    ProposalDto, RepositoriesDto, SnapshotsDto, WarningCode,
};
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};
use tokio::task::JoinHandle;

use super::*;

/// How long a test waits for an answer the server owes it.
const ANSWER: Duration = Duration::from_secs(10);

/// What the stand-in backend tells every `status`; it lists nothing.
struct Told(Result<BackendStatus, ErrorDto>);

/// The refusal of a stand-in that has nothing to tell.
fn untold() -> ErrorDto {
    ErrorDto::new(ErrorCode::Unknown, [], None)
}

#[async_trait]
impl Backend for Told {
    async fn status(&self) -> Result<BackendStatus, ErrorDto> {
        self.0.clone()
    }

    async fn plans(&self) -> Result<PlansDto, ErrorDto> {
        Err(untold())
    }

    async fn repositories(&self) -> Result<RepositoriesDto, ErrorDto> {
        Err(untold())
    }

    async fn snapshots(&self, _repository: String) -> Result<SnapshotsDto, ErrorDto> {
        Err(untold())
    }

    async fn history(
        &self,
        _plan: Option<String>,
        _after: Option<String>,
        _limit: u64,
    ) -> Result<OperationsPageDto, ErrorDto> {
        Err(untold())
    }

    async fn operation(&self, _id: String) -> Result<OperationDetailDto, ErrorDto> {
        Err(untold())
    }

    async fn summary(&self, _id: String) -> Result<OperationSummaryDto, ErrorDto> {
        Err(untold())
    }

    async fn run(&self, _plan: String) -> Result<String, ErrorDto> {
        Err(untold())
    }

    async fn cancel(&self, _id: String) -> Result<(), ErrorDto> {
        Err(untold())
    }

    async fn test_repository(&self, _repository: String) -> Result<LocationsTestDto, ErrorDto> {
        Err(untold())
    }

    async fn preview_forget(&self, _repository: String) -> Result<MaintenancePreviewDto, ErrorDto> {
        Err(untold())
    }

    async fn preview_import(
        &self,
        _content: String,
        _repository: String,
    ) -> Result<ProfileImportDto, ErrorDto> {
        Err(untold())
    }

    async fn export_profile(&self, _plan: String) -> Result<ProfileExportDto, ErrorDto> {
        Err(untold())
    }
}

fn build() -> BuildDto {
    BuildDto {
        version: "0.3.1".to_owned(),
        commit: Some("3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90".to_owned()),
        dirty: true,
        release: false,
    }
}

fn daemon() -> DaemonStatus {
    DaemonStatus {
        build: Some(BuildDto {
            version: "0.3.0".to_owned(),
            commit: None,
            dirty: false,
            release: false,
        }),
        exposure: ExposureDto::Loopback,
    }
}

fn told() -> Told {
    Told(Ok(BackendStatus {
        access: Access::ApiToken,
        daemon: Some(daemon()),
        plans: Vec::new(),
        running: Vec::new(),
    }))
}

/// The client's end of one session.
struct Client {
    input: WriteHalf<DuplexStream>,
    output: Lines<BufReader<ReadHalf<DuplexStream>>>,
    session: JoinHandle<Result<(), SessionFailed>>,
    next: u64,
    /// The messages read that answered no request [`Client::ask`] sent.
    others: Vec<Value>,
}

fn connect(backend: impl Backend) -> Client {
    connect_to(Arc::new(backend))
}

fn connect_to(backend: Arc<dyn Backend>) -> Client {
    serve(Server::new(backend, build(), english()))
}

/// A client of connect mode's server: `lists` behind every tool, the
/// proposal tools included.
fn proposing(lists: Lists) -> Client {
    serve(proposing_server(lists))
}

/// Connect mode's server over `lists`, which offers the proposal tools.
fn proposing_server(lists: Lists) -> Server {
    let lists = Arc::new(lists);
    Server::new(Arc::clone(&lists) as Arc<dyn Backend>, build(), english()).proposing(lists)
}

/// A client of one session of `server` over an in-memory pipe.
fn serve(server: Server) -> Client {
    let (client, server_end) = tokio::io::duplex(64 * 1024);
    let (server_input, server_output) = tokio::io::split(server_end);
    let session = tokio::spawn(server.serve(server_input, server_output));
    let (output, input) = tokio::io::split(client);
    Client {
        input,
        output: BufReader::new(output).lines(),
        session,
        next: 0,
        others: Vec::new(),
    }
}

/// The `_meta` every request of the current revision carries.
fn current() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

impl Client {
    async fn send(&mut self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.input.write_all(line.as_bytes()).await.unwrap();
    }

    /// The next message the server writes.
    async fn next_message(&mut self) -> Value {
        let line = tokio::time::timeout(ANSWER, self.output.next_line())
            .await
            .expect("an answer in time")
            .unwrap()
            .expect("the session still open");
        serde_json::from_str(&line).unwrap()
    }

    /// Sends `method` with `params` and returns the answer to it: the whole
    /// response, `result` or `error`.
    async fn ask(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let message = self.next_message().await;
            if message["id"] == id {
                return message;
            }
            self.others.push(message);
        }
    }

    /// A request of the current revision: `params` with its `_meta`.
    async fn ask_current(&mut self, method: &str, mut params: Value) -> Value {
        params["_meta"] = current();
        self.ask(method, params).await
    }

    async fn initialize(&mut self, revision: &str) -> Value {
        self.ask(
            "initialize",
            json!({
                "protocolVersion": revision,
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1"},
            }),
        )
        .await
    }

    async fn call_status(&mut self, arguments: Value) -> Value {
        self.ask_current(
            "tools/call",
            json!({"name": "status", "arguments": arguments}),
        )
        .await
    }
}

#[tokio::test(start_paused = true)]
async fn discovery_names_the_three_revisions_served() {
    let mut client = connect(told());
    let answer = client.ask_current("server/discover", json!({})).await;
    assert_eq!(
        answer["result"]["supportedVersions"],
        json!(["2025-06-18", "2025-11-25", "2026-07-28"]),
        "{answer}"
    );
    assert!(
        answer["result"]["capabilities"]["tools"].is_object(),
        "{answer}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_handshake_agrees_to_each_earlier_revision_served() {
    for revision in ["2025-06-18", "2025-11-25"] {
        let mut client = connect(told());
        let answer = client.initialize(revision).await;
        assert_eq!(answer["result"]["protocolVersion"], revision, "{answer}");
        assert_eq!(answer["result"]["serverInfo"]["name"], "ladderback");
        assert_eq!(
            answer["result"]["serverInfo"]["version"],
            build().display().as_str()
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_handshake_of_an_older_revision_is_answered_with_one_served() {
    let mut client = connect(told());
    let answer = client.initialize("2025-03-26").await;
    assert_eq!(
        answer["result"]["protocolVersion"], "2025-11-25",
        "{answer}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_request_of_an_older_revision_without_a_handshake_is_refused() {
    let mut client = connect(told());
    let mut meta = current();
    meta["io.modelcontextprotocol/protocolVersion"] = json!("2025-03-26");
    let answer = client.ask("tools/list", json!({"_meta": meta})).await;
    assert!(answer["error"].is_object(), "{answer}");
}

/// The tools `tools/list` gives, as `tests/golden/tools-list.json` holds
/// them: a change to a tool's name, description or schemas changes the file,
/// so a review sees it.
#[tokio::test(start_paused = true)]
async fn the_tools_listed_are_the_golden_files() {
    let mut client = proposing(Lists::default());
    let answer = client.ask_current("tools/list", json!({})).await;
    let tools = &answer["result"]["tools"];
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/tools-list.json");
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(&golden).expect("the golden file")).unwrap();
    assert!(
        *tools == expected,
        "the tools listed aren't the golden file's; if the change is meant, write this to {}:\n{}",
        golden.display(),
        serde_json::to_string_pretty(tools).unwrap()
    );
    let names: Vec<&str> = tools
        .as_array()
        .expect("a list of tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(
        names,
        [
            "status",
            "list_plans",
            "list_repositories",
            "list_snapshots",
            "list_operations",
            "get_operation",
            "run_plan",
            "wait_operation",
            "cancel_operation",
            "test_repository",
            "preview_forget",
            "preview_import",
            "export_profile",
            "propose_setup",
            "get_proposal",
            "withdraw_proposal",
        ]
    );
    for tool in tools.as_array().expect("a list of tools") {
        // Starting and stopping a backup changes what Ladderback does, never
        // what it keeps; testing a repository keeps nothing but needs the
        // `run` scope; proposing and withdrawing change a proposal, never
        // the configuration; everything else only reads.
        let reads = !matches!(
            tool["name"].as_str(),
            Some(
                "run_plan"
                    | "cancel_operation"
                    | "test_repository"
                    | "propose_setup"
                    | "withdraw_proposal"
            )
        );
        assert_eq!(tool["annotations"]["readOnlyHint"], reads, "{tool}");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{tool}");
        assert_eq!(tool["outputSchema"]["type"], "object", "{tool}");
    }
    assert_eq!(
        tools[0]["inputSchema"],
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    );
}

/// Some clients turn a tool's input schema into a function declaration of a
/// narrower schema language (§4.6): each argument has one type, never a list
/// with null, no `format` and no `minimum`, a bound being said in its
/// description; an optional one is left out of `required`. The tools that
/// take an operation name it `operation`, as `run_plan` and
/// `cancel_operation` return it.
#[tokio::test(start_paused = true)]
async fn each_argument_has_one_type_no_format_or_minimum_and_only_those_needed_are_required() {
    let mut client = proposing(Lists::default());
    let answer = client.ask_current("tools/list", json!({})).await;
    let mut required = HashMap::new();
    for tool in answer["result"]["tools"]
        .as_array()
        .expect("a list of tools")
    {
        let name = tool["name"].as_str().expect("a name");
        let schema = &tool["inputSchema"];
        let arguments = schema["properties"].as_object().expect("its arguments");
        for (argument, argument_schema) in arguments {
            assert!(
                argument_schema["type"].is_string(),
                "{name}.{argument}: {schema}"
            );
            assert!(
                argument_schema.get("format").is_none(),
                "{name}.{argument}: {schema}"
            );
            assert!(
                argument_schema.get("minimum").is_none(),
                "{name}.{argument}: {schema}"
            );
        }
        let needed: Vec<&str> = schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|argument| argument.as_str().expect("an argument's name"))
            .collect();
        required.insert(name, needed);
    }
    let expected: HashMap<&str, Vec<&str>> = [
        ("status", vec![]),
        ("list_plans", vec![]),
        ("list_repositories", vec![]),
        ("list_snapshots", vec!["repository"]),
        ("list_operations", vec![]),
        ("get_operation", vec!["operation"]),
        ("run_plan", vec!["plan"]),
        ("wait_operation", vec!["operation"]),
        ("cancel_operation", vec!["operation"]),
        ("test_repository", vec!["repository"]),
        ("preview_forget", vec!["repository"]),
        ("preview_import", vec!["content", "repository"]),
        ("export_profile", vec!["plan"]),
        ("propose_setup", vec!["changes"]),
        ("get_proposal", vec!["proposal"]),
        ("withdraw_proposal", vec!["proposal"]),
    ]
    .into_iter()
    .collect();
    assert_eq!(required, expected);
}

#[tokio::test(start_paused = true)]
async fn status_names_this_programs_build_field_by_field_and_what_the_backend_told() {
    let mut client = connect(told());
    let answer = client.call_status(json!({})).await;
    let result = &answer["result"];
    assert_eq!(result["isError"], false, "{answer}");
    assert_eq!(
        result["structuredContent"],
        json!({
            "build": {
                "version": "0.3.1",
                "commit": "3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90",
                "dirty": true,
                "release": false,
            },
            "access": "api-token",
            "daemon": {
                "build": {"version": "0.3.0", "commit": null, "dirty": false, "release": false},
                "exposure": "loopback",
            },
            "plans": [],
            "running": [],
        })
    );
    let text = result["content"][0]["text"]
        .as_str()
        .expect("the result as text");
    let mirrored: Value = serde_json::from_str(text).unwrap();
    assert_eq!(mirrored, result["structuredContent"]);
}

#[tokio::test(start_paused = true)]
async fn a_refusal_from_the_backend_is_a_result_marked_as_an_error() {
    let mut client = connect(Told(Err(ErrorDto::new(
        ErrorCode::SignInRequired,
        [],
        None,
    ))));
    let answer = client.call_status(json!({})).await;
    assert_eq!(refusal(&answer), json!({"code": "sign-in-required"}));
}

/// The `{code, args}` of a refusal: a result marked as an error, with no
/// `structuredContent`, whose first text content holds it as JSON.
fn refusal(answer: &Value) -> Value {
    let result = &answer["result"];
    assert_eq!(result["isError"], true, "{answer}");
    assert!(result.get("structuredContent").is_none(), "{answer}");
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text: {answer}"));
    serde_json::from_str(text).unwrap_or_else(|error| panic!("{error}: {text}"))
}

/// The sentence a refusal says after its `{code, args}`: its second and last
/// text content.
fn said(answer: &Value) -> &str {
    let content = answer["result"]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("no content: {answer}"));
    assert_eq!(content.len(), 2, "{answer}");
    assert_eq!(content[1]["type"], "text", "{answer}");
    content[1]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no sentence: {answer}"))
}

/// The localizer of a server whose environment asks for no locale.
fn english() -> Localizer {
    Localizer::new(&[])
}

/// The localizer of a server whose environment asks for the pseudolocale
/// `en-XA` (§8.4, "Checking your text").
fn pseudo() -> Localizer {
    let environment = l10n::PosixEnv::from_lookup(|name| match name {
        "LANG" => Some("en_US.UTF-8".to_owned()),
        "LANGUAGE" => Some("en-XA".to_owned()),
        _ => None,
    });
    Localizer::new(&environment.preferences())
}

/// What the catalog of `localizer` says of the error `code` with `args`
/// and `detail`.
fn catalog(
    localizer: &Localizer,
    code: ErrorCode,
    args: &[(&str, api::MsgArg)],
    detail: Option<&str>,
) -> String {
    localizer.error(code, args, detail).into_text()
}

/// A refusal keeps its `{code, args}` for a client to branch on, and says
/// after it, in the catalog's words, what the agent can do next (§4.6).
#[tokio::test(start_paused = true)]
async fn a_refusal_says_the_catalogs_sentence_after_its_code() {
    let scope = [("scope", api::MsgArg::Text("run".to_owned()))];
    let refused = ErrorDto::new(ErrorCode::ApiTokenScope, scope.clone(), None);
    let mut client = connect(Told(Err(refused.clone())));
    let answer = client.call_status(json!({})).await;
    assert_eq!(refusal(&answer), serde_json::to_value(&refused).unwrap());
    assert_eq!(
        said(&answer),
        catalog(&english(), ErrorCode::ApiTokenScope, &scope, None)
    );
}

/// The sentence carries the sender's own words where the catalog puts them.
#[tokio::test(start_paused = true)]
async fn a_refusals_sentence_holds_its_detail() {
    let refused = ErrorDto::new(
        ErrorCode::OperationFailed,
        [],
        Some("the disk is full".to_owned()),
    );
    let mut client = connect(Told(Err(refused)));
    let answer = client.call_status(json!({})).await;
    let expected = catalog(
        &english(),
        ErrorCode::OperationFailed,
        &[],
        Some("the disk is full"),
    );
    assert!(expected.contains("the disk is full"), "{expected}");
    assert_eq!(said(&answer), expected);
}

/// The sentence is in the server's locale, which the environment it runs in
/// chose, never fixed English.
#[test]
fn a_refusal_is_said_in_the_locale_the_server_was_given() {
    let plan = [("plan", api::MsgArg::Text("home".to_owned()))];
    let error = ErrorDto::new(ErrorCode::PlanUnknown, plan.clone(), None);
    let server = Server::new(Arc::new(told()), build(), pseudo());
    let sentence = server.sentence(&error);
    assert_eq!(
        sentence,
        catalog(&pseudo(), ErrorCode::PlanUnknown, &plan, None)
    );
    assert_ne!(
        sentence,
        catalog(&english(), ErrorCode::PlanUnknown, &plan, None)
    );
    assert!(sentence.contains("home"), "{sentence}");
}

#[tokio::test(start_paused = true)]
async fn an_unknown_tool_is_invalid_params() {
    let mut client = connect(told());
    let answer = client
        .ask_current("tools/call", json!({"name": "restore", "arguments": {}}))
        .await;
    assert_eq!(answer["error"]["code"], -32602, "{answer}");
}

/// Arguments that aren't an object aren't a call of any tool's shape:
/// invalid params, a protocol error, before a tool reads them.
#[tokio::test(start_paused = true)]
async fn arguments_that_arent_an_object_are_invalid_params() {
    let lists = Lists::default();
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    for arguments in [json!("home"), json!(["home"]), json!(7)] {
        let answer = client
            .ask_current(
                "tools/call",
                json!({"name": "run_plan", "arguments": arguments}),
            )
            .await;
        assert_eq!(answer["error"]["code"], -32602, "{answer}");
    }
    assert!(asked.lock().unwrap().is_empty(), "{:?}", asked.lock());
}

/// A method the server lacks stays that, apart from a malformed tool call.
#[tokio::test(start_paused = true)]
async fn a_method_the_server_lacks_is_method_not_found() {
    let mut client = connect(told());
    let answer = client.ask_current("tools/restore", json!({})).await;
    assert_eq!(answer["error"]["code"], -32601, "{answer}");
}

/// The `invalid-arguments` refusal of `field`, for `problem`.
fn invalid(field: &str, problem: &str) -> Value {
    json!({
        "code": "invalid-arguments",
        "args": {
            "field": {"type": "text", "value": field},
            "problem": {"type": "text", "value": problem},
        },
    })
}

#[tokio::test(start_paused = true)]
async fn an_argument_to_status_is_refused_as_one_it_doesnt_take() {
    let mut client = connect(told());
    let answer = client.call_status(json!({"plan": "home"})).await;
    assert_eq!(refusal(&answer), invalid("plan", "unknown-key"));
}

/// A `tools/list` request with the ID `id`, padded so that its line, the
/// line end included, is `bytes` long.
fn padded(id: u64, bytes: usize) -> Value {
    let mut meta = current();
    meta["padding"] = json!("");
    let mut message =
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {"_meta": meta}});
    let unpadded = message.to_string().len() + 1;
    message["params"]["_meta"]["padding"] = json!("x".repeat(bytes - unpadded));
    message
}

#[tokio::test(start_paused = true)]
async fn a_message_of_one_mebibyte_is_read() {
    let mut client = connect(told());
    client.send(&padded(99, 1024 * 1024)).await;
    let answer = client.next_message().await;
    assert_eq!(answer["id"], 99, "{answer}");
    assert!(answer["result"]["tools"].is_array(), "{answer}");
}

#[tokio::test(start_paused = true)]
async fn a_message_over_the_limit_is_dropped_and_the_session_goes_on() {
    let mut client = connect(told());
    client.send(&padded(99, 1024 * 1024 + 1)).await;
    for _ in 0..2 {
        let answer = client.ask_current("tools/list", json!({})).await;
        assert!(answer["result"]["tools"].is_array(), "{answer}");
    }
    assert!(
        !client
            .others
            .iter()
            .any(|message| message["id"] == 99 && !message["result"].is_null()),
        "the request over the limit was answered: {:?}",
        client.others
    );
}

#[tokio::test(start_paused = true)]
async fn the_session_ends_when_the_client_closes_its_input() {
    let mut client = connect(told());
    client.ask_current("tools/list", json!({})).await;
    client.input.shutdown().await.unwrap();
    let ended = tokio::time::timeout(ANSWER, client.session).await;
    assert!(matches!(ended, Ok(Ok(Ok(())))), "{ended:?}");
}

#[tokio::test(start_paused = true)]
async fn input_closed_before_any_request_ends_the_session_quietly() {
    let mut client = connect(told());
    client.input.shutdown().await.unwrap();
    let ended = tokio::time::timeout(ANSWER, client.session).await;
    assert!(matches!(ended, Ok(Ok(Ok(())))), "{ended:?}");
}

/// A backend that lists what it was given, or refuses every list with
/// `refusal`, and notes what each call asked.
#[derive(Default)]
struct Lists {
    plans: Vec<PlanSummaryDto>,
    repositories: Vec<api::RepositoryDto>,
    snapshots: Vec<api::SnapshotDto>,
    page: Option<OperationsPageDto>,
    detail: Option<OperationDetailDto>,
    preview: Option<MaintenancePreviewDto>,
    refusal: Option<ErrorDto>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Lists {
    fn told<T: Clone>(&self, what: &T) -> Result<T, ErrorDto> {
        match &self.refusal {
            Some(refusal) => Err(refusal.clone()),
            None => Ok(what.clone()),
        }
    }

    fn ask(&self, what: String) {
        self.asked.lock().unwrap().push(what);
    }
}

/// The proposal a stand-in daemon keeps for every proposal tool: pending,
/// with one warning of the submission checks.
fn a_proposal() -> ProposalDto {
    ProposalDto {
        id: "0197a000-0000-7000-8000-000000000001".to_owned(),
        proposed_by: Some("assistant".to_owned()),
        status: api::ProposalStatusDto::Pending,
        created_at: "2026-11-18T10:00:00Z".to_owned(),
        expires_at: "2026-11-20T10:00:00Z".to_owned(),
        changes: vec![api::ProposalChangeDto::RemovePlan {
            plan: "home".to_owned(),
        }],
        rationale: "Nightly.".to_owned(),
        decided_at: None,
        reasons: Vec::new(),
        warnings: vec![api::ProposalWarningDto::GlobNeverMatches {
            change: 1,
            glob: "!x".to_owned(),
        }],
    }
}

#[async_trait]
impl Proposals for Lists {
    async fn propose(&self, proposal: NewProposalDto) -> Result<ProposalDto, ErrorDto> {
        self.ask(format!(
            "propose {}",
            serde_json::to_string(&proposal).unwrap()
        ));
        self.told(&a_proposal())
    }

    async fn proposal(&self, id: String) -> Result<ProposalDto, ErrorDto> {
        self.ask(format!("proposal {id}"));
        self.told(&a_proposal())
    }

    async fn withdraw(&self, id: String) -> Result<ProposalDto, ErrorDto> {
        self.ask(format!("withdraw {id}"));
        self.told(&a_proposal())
    }
}

#[async_trait]
impl Backend for Lists {
    async fn status(&self) -> Result<BackendStatus, ErrorDto> {
        Err(untold())
    }

    async fn plans(&self) -> Result<PlansDto, ErrorDto> {
        self.told(&PlansDto {
            plans: self.plans.clone(),
        })
    }

    async fn repositories(&self) -> Result<RepositoriesDto, ErrorDto> {
        self.told(&RepositoriesDto {
            repositories: self.repositories.clone(),
        })
    }

    async fn snapshots(&self, repository: String) -> Result<SnapshotsDto, ErrorDto> {
        self.ask(format!("snapshots of {repository}"));
        self.told(&SnapshotsDto {
            snapshots: self.snapshots.clone(),
        })
    }

    async fn history(
        &self,
        plan: Option<String>,
        after: Option<String>,
        limit: u64,
    ) -> Result<OperationsPageDto, ErrorDto> {
        self.ask(format!("history of {plan:?} after {after:?}, {limit}"));
        self.told(self.page.as_ref().unwrap_or(&OperationsPageDto {
            operations: Vec::new(),
            next: None,
        }))
    }

    async fn operation(&self, id: String) -> Result<OperationDetailDto, ErrorDto> {
        self.ask(format!("operation {id}"));
        self.told(self.detail.as_ref().expect("a detail to tell"))
    }

    async fn summary(&self, id: String) -> Result<OperationSummaryDto, ErrorDto> {
        self.ask(format!("summary of {id}"));
        self.told(&self.detail.as_ref().expect("a detail to tell").operation)
    }

    async fn run(&self, plan: String) -> Result<String, ErrorDto> {
        self.ask(format!("run {plan}"));
        self.told(&"started".to_owned())
    }

    async fn cancel(&self, id: String) -> Result<(), ErrorDto> {
        self.ask(format!("cancel {id}"));
        self.told(&())
    }

    async fn test_repository(&self, repository: String) -> Result<LocationsTestDto, ErrorDto> {
        self.ask(format!("test {repository}"));
        self.told(&LocationsTestDto {
            locations: vec![api::LocationTriedDto {
                position: 1,
                address: "/srv/nas".to_owned(),
                outcome: "used".to_owned(),
                source: None,
            }],
        })
    }

    async fn preview_forget(&self, repository: String) -> Result<MaintenancePreviewDto, ErrorDto> {
        self.ask(format!("forget preview of {repository}"));
        self.told(self.preview.as_ref().expect("a preview to tell"))
    }

    async fn preview_import(
        &self,
        content: String,
        repository: String,
    ) -> Result<ProfileImportDto, ErrorDto> {
        self.ask(format!("import preview of {content:?} as {repository}"));
        self.told(&ProfileImportDto {
            repository,
            plans: vec!["home".to_owned()],
            notices: Vec::new(),
        })
    }

    async fn export_profile(&self, plan: String) -> Result<ProfileExportDto, ErrorDto> {
        self.ask(format!("export {plan}"));
        self.told(&ProfileExportDto {
            text: "[repository]\n".to_owned(),
            notices: Vec::new(),
        })
    }
}

fn plan(id: &str) -> PlanSummaryDto {
    serde_json::from_value(
        json!({"id": id, "repository": "nas", "scheduled": false, "next-run": null}),
    )
    .unwrap()
}

fn plans(count: usize) -> Vec<PlanSummaryDto> {
    (0..count).map(|n| plan(&format!("plan-{n:03}"))).collect()
}

fn repository(alias: &str) -> api::RepositoryDto {
    serde_json::from_value(json!({
        "alias": alias,
        "repository": format!("/srv/backups/{alias}"),
        "password": {"source": "env", "name": "DEMO_PASSWORD"},
    }))
    .unwrap()
}

fn snapshot(n: u8) -> api::SnapshotDto {
    serde_json::from_value(json!({
        "id": format!("{n:064x}"),
        "time": format!("2026-11-{:02}T10:00:00Z", 20 - n),
        "hostname": "nas",
        "label": "",
        "tags": [],
        "paths": ["/home/demo"],
    }))
    .unwrap()
}

fn summary(id: u8) -> OperationSummaryDto {
    serde_json::from_value(json!({
        "id": format!("00000000-0000-7000-8000-0000000000{id:02}"),
        "plan": "home",
        "kind": "backup",
        "status": "succeeded-with-warnings",
        "queued-at": "2026-11-14T10:00:00Z",
        "started-at": "2026-11-14T10:00:01Z",
        "finished-at": "2026-11-14T10:01:00Z",
        "warning-codes": ["source-unreadable"],
        "failure-code": null,
        "result": null,
    }))
    .unwrap()
}

fn warning(n: usize) -> OperationWarningDto {
    OperationWarningDto {
        code: WarningCode::SourceUnreadable,
        item: Some(format!("/home/demo/file-{n}")),
        during: "backup".to_owned(),
        detail: "permission denied".to_owned(),
    }
}

impl Client {
    async fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.ask_current("tools/call", json!({"name": tool, "arguments": arguments}))
            .await
    }

    /// The `structuredContent` of a result that isn't an error.
    async fn listed(&mut self, tool: &str, arguments: Value) -> Value {
        let answer = self.call(tool, arguments).await;
        assert_eq!(answer["result"]["isError"], false, "{answer}");
        answer["result"]["structuredContent"].clone()
    }
}

/// The IDs of `content`'s `key` list, by the field `id`.
fn ids<'a>(content: &'a Value, key: &str, id: &str) -> Vec<&'a str> {
    content[key]
        .as_array()
        .unwrap_or_else(|| panic!("no {key}: {content}"))
        .iter()
        .map(|item| item[id].as_str().expect("an ID"))
        .collect()
}

#[tokio::test(start_paused = true)]
async fn list_plans_lists_a_page_of_the_limit_and_the_next_continues_after_its_cursor() {
    let mut client = connect(Lists {
        plans: plans(3),
        ..Lists::default()
    });
    let first = client.listed("list_plans", json!({"limit": 2})).await;
    assert_eq!(ids(&first, "plans", "id"), ["plan-000", "plan-001"]);
    assert_eq!(first["next"], "plan-001");
    let second = client
        .listed("list_plans", json!({"limit": 2, "cursor": "plan-001"}))
        .await;
    assert_eq!(ids(&second, "plans", "id"), ["plan-002"]);
    assert_eq!(second["next"], Value::Null);
}

#[tokio::test(start_paused = true)]
async fn a_page_of_exactly_what_is_left_has_no_next() {
    let mut client = connect(Lists {
        plans: plans(2),
        ..Lists::default()
    });
    let page = client.listed("list_plans", json!({"limit": 2})).await;
    assert_eq!(ids(&page, "plans", "id"), ["plan-000", "plan-001"]);
    assert_eq!(page["next"], Value::Null);
}

#[tokio::test(start_paused = true)]
async fn a_page_lists_fifty_when_the_call_names_no_limit() {
    let mut client = connect(Lists {
        plans: plans(60),
        ..Lists::default()
    });
    let page = client.listed("list_plans", json!({})).await;
    assert_eq!(ids(&page, "plans", "id").len(), 50);
    assert_eq!(page["next"], "plan-049");
}

#[tokio::test(start_paused = true)]
async fn a_limit_over_the_largest_page_lists_two_hundred_and_one_under_one_lists_one() {
    let mut client = connect(Lists {
        plans: plans(250),
        ..Lists::default()
    });
    let page = client
        .listed("list_plans", json!({"limit": 1_000_000_000_000_u64}))
        .await;
    assert_eq!(ids(&page, "plans", "id").len(), 200);
    assert_eq!(page["next"], "plan-199");
    let page = client.listed("list_plans", json!({"limit": 0})).await;
    assert_eq!(ids(&page, "plans", "id"), ["plan-000"]);
}

#[tokio::test(start_paused = true)]
async fn list_repositories_pages_by_alias_and_refuses_a_cursor_that_names_none() {
    let mut client = connect(Lists {
        repositories: vec![repository("nas"), repository("offsite")],
        ..Lists::default()
    });
    let first = client
        .listed("list_repositories", json!({"limit": 1}))
        .await;
    assert_eq!(ids(&first, "repositories", "alias"), ["nas"]);
    assert_eq!(first["next"], "nas");
    let second = client
        .listed("list_repositories", json!({"cursor": "nas"}))
        .await;
    assert_eq!(ids(&second, "repositories", "alias"), ["offsite"]);
    assert_eq!(second["next"], Value::Null);
    let answer = client
        .call("list_repositories", json!({"cursor": "gone"}))
        .await;
    assert_eq!(
        refusal(&answer),
        json!({"code": "invalid-cursor", "args": {"cursor": {"type": "text", "value": "gone"}}})
    );
}

#[tokio::test(start_paused = true)]
async fn list_snapshots_reads_the_repository_named_and_pages_by_snapshot_id() {
    let lists = Lists {
        snapshots: vec![snapshot(1), snapshot(2), snapshot(3)],
        ..Lists::default()
    };
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    let first = client
        .listed("list_snapshots", json!({"repository": "nas", "limit": 2}))
        .await;
    let expected = [format!("{:064x}", 1), format!("{:064x}", 2)];
    assert_eq!(ids(&first, "snapshots", "id"), expected);
    assert_eq!(first["next"], expected[1].as_str());
    let second = client
        .listed(
            "list_snapshots",
            json!({"repository": "nas", "cursor": expected[1]}),
        )
        .await;
    assert_eq!(ids(&second, "snapshots", "id"), [format!("{:064x}", 3)]);
    assert_eq!(second["next"], Value::Null);
    assert_eq!(
        *asked.lock().unwrap(),
        ["snapshots of nas", "snapshots of nas"]
    );
}

/// Snapshot `n` of `plan`, taken on `host`, tagged as a plan's backup is,
/// after a tag of the user's.
fn snapshot_of(n: u8, plan: &str, host: &str) -> api::SnapshotDto {
    let mut snapshot = snapshot(n);
    snapshot.hostname = host.to_owned();
    snapshot.tags = vec![
        "nightly".to_owned(),
        format!("ladderback-plan:{plan}"),
        "ladderback-agent:demo".to_owned(),
    ];
    snapshot
}

#[tokio::test(start_paused = true)]
async fn list_snapshots_lists_only_those_of_the_plan_and_host_named_and_pages_what_it_keeps() {
    let lists = Lists {
        snapshots: vec![
            snapshot_of(1, "home", "laptop"),
            snapshot_of(2, "work", "laptop"),
            snapshot_of(3, "home", "desktop"),
            snapshot_of(4, "home-old", "laptop"),
            snapshot(5),
            snapshot_of(6, "home", "laptop"),
        ],
        ..Lists::default()
    };
    let mut client = connect(lists);
    let id = |n: u8| format!("{n:064x}");
    let listing = |arguments: Value| {
        let mut all = json!({"repository": "nas"});
        all.as_object_mut()
            .unwrap()
            .extend(arguments.as_object().unwrap().clone());
        all
    };

    let of_home = client
        .listed("list_snapshots", listing(json!({"plan": "home"})))
        .await;
    assert_eq!(ids(&of_home, "snapshots", "id"), [id(1), id(3), id(6)]);
    let on_laptop = client
        .listed("list_snapshots", listing(json!({"host": "laptop"})))
        .await;
    assert_eq!(
        ids(&on_laptop, "snapshots", "id"),
        [id(1), id(2), id(4), id(6)]
    );
    // Both, a page at a time: the cursor is a snapshot the filters keep.
    let first = client
        .listed(
            "list_snapshots",
            listing(json!({"plan": "home", "host": "laptop", "limit": 1})),
        )
        .await;
    assert_eq!(ids(&first, "snapshots", "id"), [id(1)]);
    assert_eq!(first["next"], id(1).as_str());
    let second = client
        .listed(
            "list_snapshots",
            listing(json!({"plan": "home", "host": "laptop", "cursor": id(1)})),
        )
        .await;
    assert_eq!(ids(&second, "snapshots", "id"), [id(6)]);
    assert_eq!(second["next"], Value::Null);
    // A host is named exactly; a plan with no snapshots lists none.
    for arguments in [json!({"host": "LAPTOP"}), json!({"plan": "photos"})] {
        let none = client
            .listed("list_snapshots", listing(arguments.clone()))
            .await;
        assert_eq!(none, json!({"snapshots": [], "next": null}), "{arguments}");
    }
    // A cursor the filters don't keep names no snapshot of the list.
    let answer = client
        .call(
            "list_snapshots",
            listing(json!({"plan": "home", "cursor": id(2)})),
        )
        .await;
    assert_eq!(refusal(&answer)["code"], "invalid-cursor");
}

#[test]
fn a_snapshots_plan_is_the_one_its_first_plan_tag_names() {
    let mut snapshot = snapshot(1);
    snapshot.tags = vec![
        "my-ladderback-plan:work".to_owned(),
        "ladderback-plan:home".to_owned(),
        "ladderback-plan:work".to_owned(),
    ];
    let of = |plan: &str| SnapshotFilter {
        plan: Some(plan.to_owned()),
        host: None,
    };
    assert!(of("home").keeps(&snapshot));
    assert!(!of("work").keeps(&snapshot));
    assert!(!of("hom").keeps(&snapshot));
    assert!(SnapshotFilter::default().keeps(&snapshot));
    let on = |host: &str| SnapshotFilter {
        plan: None,
        host: Some(host.to_owned()),
    };
    assert!(on("nas").keeps(&snapshot));
    assert!(!on("na").keeps(&snapshot));
}

#[tokio::test(start_paused = true)]
async fn list_operations_asks_for_the_plan_the_cursor_and_the_clamped_limit() {
    let page = OperationsPageDto {
        operations: vec![summary(2)],
        next: Some(summary(2).id),
    };
    let lists = Lists {
        page: Some(page.clone()),
        ..Lists::default()
    };
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    let cursor = summary(3).id;
    let listed = client
        .listed(
            "list_operations",
            json!({"plan": "home", "cursor": cursor, "limit": 999}),
        )
        .await;
    assert_eq!(listed, serde_json::to_value(&page).unwrap());
    client.listed("list_operations", json!({})).await;
    assert_eq!(
        *asked.lock().unwrap(),
        [
            format!("history of Some(\"home\") after Some({cursor:?}), 200"),
            "history of None after None, 50".to_owned(),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn get_operation_lists_the_first_hundred_warnings_and_counts_the_rest() {
    let lists = Lists {
        detail: Some(OperationDetailDto {
            operation: summary(1),
            warnings: (0..150).map(warning).collect(),
        }),
        ..Lists::default()
    };
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    let id = summary(1).id;
    let content = client
        .listed("get_operation", json!({"operation": id}))
        .await;
    assert_eq!(
        content["operation"],
        serde_json::to_value(summary(1)).unwrap()
    );
    let warnings = content["warnings"].as_array().expect("its warnings");
    assert_eq!(warnings.len(), 100);
    assert_eq!(warnings[99], serde_json::to_value(warning(99)).unwrap());
    assert_eq!(content["unlisted-warnings"], 50);
    assert_eq!(*asked.lock().unwrap(), [format!("operation {id}")]);
}

#[tokio::test(start_paused = true)]
async fn get_operation_with_every_warning_listed_counts_none_unlisted() {
    let mut client = connect(Lists {
        detail: Some(OperationDetailDto {
            operation: summary(1),
            warnings: (0..100).map(warning).collect(),
        }),
        ..Lists::default()
    });
    let content = client
        .listed("get_operation", json!({"operation": summary(1).id}))
        .await;
    assert_eq!(content["warnings"].as_array().map(Vec::len), Some(100));
    assert_eq!(content["unlisted-warnings"], 0);
}

/// An argument a tool can't take is the tool's refusal, which the agent
/// reads, naming the argument and what is wrong with it but never the value
/// given; and the backend is asked nothing.
#[tokio::test(start_paused = true)]
async fn an_argument_a_tool_cant_take_is_refused_naming_it() {
    let lists = Lists::default();
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    for (tool, arguments, field, problem) in [
        ("get_operation", json!({}), "operation", "missing"),
        ("get_operation", json!({"id": "x"}), "id", "unknown-key"),
        ("list_plans", json!({"plan": "home"}), "plan", "unknown-key"),
        (
            "list_repositories",
            json!({"limit": "ten"}),
            "limit",
            "invalid",
        ),
        (
            "list_snapshots",
            json!({"limit": 2}),
            "repository",
            "missing",
        ),
        (
            "list_snapshots",
            json!({"repository": "nas", "host": ["canary-host"]}),
            "host",
            "invalid",
        ),
        (
            "list_operations",
            json!({"repository": "nas"}),
            "repository",
            "unknown-key",
        ),
        ("list_operations", json!({"limit": -1}), "limit", "invalid"),
        ("list_operations", json!({"limit": 2.5}), "limit", "invalid"),
        // Null for an argument it needs is no value for it; for an optional
        // one it is the argument left out, so the one it needs is missing.
        (
            "get_operation",
            json!({"operation": null}),
            "operation",
            "invalid",
        ),
        (
            "list_snapshots",
            json!({"limit": null}),
            "repository",
            "missing",
        ),
        ("test_repository", json!({}), "repository", "missing"),
        (
            "preview_forget",
            json!({"repository": "nas", "selection": "x"}),
            "selection",
            "unknown-key",
        ),
        (
            "preview_import",
            json!({"content": "[repository]\n"}),
            "repository",
            "missing",
        ),
        (
            "preview_import",
            json!({"path": "/etc/rustic/nas.toml", "repository": "nas"}),
            "path",
            "unknown-key",
        ),
        ("export_profile", json!({"plan": 7}), "plan", "invalid"),
    ] {
        let answer = client.call(tool, arguments).await;
        assert_eq!(refusal(&answer), invalid(field, problem), "{tool}");
        assert!(!answer.to_string().contains("canary"), "{answer}");
    }
    assert!(asked.lock().unwrap().is_empty(), "{:?}", asked.lock());
}

#[tokio::test(start_paused = true)]
async fn every_tool_but_status_hands_on_a_refusal_from_the_backend_as_an_error() {
    let told = ErrorDto::new(
        ErrorCode::RepositoryUnknown,
        [("alias", api::MsgArg::Text("nas".to_owned()))],
        None,
    );
    let mut client = proposing(Lists {
        refusal: Some(told.clone()),
        detail: Some(OperationDetailDto {
            operation: summary(1),
            warnings: Vec::new(),
        }),
        preview: Some(forget_preview()),
        ..Lists::default()
    });
    for (tool, arguments) in [
        ("list_plans", json!({})),
        ("list_repositories", json!({})),
        ("list_snapshots", json!({"repository": "nas"})),
        ("list_operations", json!({})),
        ("get_operation", json!({"operation": summary(1).id})),
        ("run_plan", json!({"plan": "home"})),
        ("wait_operation", json!({"operation": summary(1).id})),
        ("cancel_operation", json!({"operation": summary(1).id})),
        ("test_repository", json!({"repository": "nas"})),
        ("preview_forget", json!({"repository": "nas"})),
        (
            "preview_import",
            json!({"content": "[repository]\n", "repository": "nas"}),
        ),
        ("export_profile", json!({"plan": "home"})),
        ("propose_setup", json!({"changes": [remove_home()]})),
        ("get_proposal", json!({"proposal": "p"})),
        ("withdraw_proposal", json!({"proposal": "p"})),
    ] {
        let answer = client.call(tool, arguments).await;
        assert_eq!(
            refusal(&answer),
            serde_json::to_value(&told).unwrap(),
            "{tool}"
        );
    }
}

/// The golden file's output schema of each tool, by name.
fn golden_output_schemas() -> HashMap<String, Value> {
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/tools-list.json");
    let tools: Value =
        serde_json::from_str(&std::fs::read_to_string(golden).expect("the golden file")).unwrap();
    tools
        .as_array()
        .expect("a list of tools")
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().expect("a name").to_owned(),
                tool["outputSchema"].clone(),
            )
        })
        .collect()
}

/// Whether `content` has every key `schema` requires and, where it takes no
/// others, only those it names: what a client checks first of
/// `structuredContent` against a tool's output schema.
fn keys_conform(content: &Value, schema: &Value) -> Result<(), String> {
    let keys = content
        .as_object()
        .ok_or_else(|| format!("not an object: {content}"))?;
    for required in schema["required"].as_array().into_iter().flatten() {
        let required = required.as_str().unwrap_or_default();
        if !keys.contains_key(required) {
            return Err(format!("no {required:?} in {content}"));
        }
    }
    if schema["additionalProperties"] == false {
        let named = schema["properties"].as_object();
        if let Some(other) = keys
            .keys()
            .find(|key| !named.is_some_and(|named| named.contains_key(*key)))
        {
            return Err(format!("{other:?} isn't in the schema: {content}"));
        }
    }
    Ok(())
}

/// A client such as the official TypeScript SDK checks `structuredContent`
/// against the tool's output schema whether the result is an error or not,
/// and throws when it doesn't conform, so a refusal carries none: no schema
/// declares a refusal's shape.
#[tokio::test(start_paused = true)]
async fn no_refusal_of_any_tool_carries_structured_content() {
    let schemas = golden_output_schemas();
    let told = ErrorDto::new(
        ErrorCode::PlanUnknown,
        [("plan", api::MsgArg::Text("home".to_owned()))],
        None,
    );
    let mut status = connect(Told(Err(told.clone())));
    let mut client = proposing(Lists {
        refusal: Some(told.clone()),
        detail: Some(OperationDetailDto {
            operation: summary(1),
            warnings: Vec::new(),
        }),
        preview: Some(forget_preview()),
        ..Lists::default()
    });
    let mut checked = Vec::new();
    for (tool, arguments) in every_tool_called() {
        let answer = if tool == "status" {
            status.call(tool, arguments).await
        } else {
            client.call(tool, arguments).await
        };
        let result = &answer["result"];
        assert_eq!(result["isError"], true, "{tool}: {answer}");
        assert_eq!(result.get("structuredContent"), None, "{tool}: {answer}");
        assert_eq!(
            refusal(&answer),
            serde_json::to_value(&told).unwrap(),
            "{tool}"
        );
        checked.push(tool);
    }
    assert_eq!(checked.len(), schemas.len(), "every tool listed is checked");
}

/// Each tool, with arguments it takes.
fn every_tool_called() -> [(&'static str, Value); 16] {
    [
        ("status", json!({})),
        ("list_plans", json!({})),
        ("list_repositories", json!({})),
        ("list_snapshots", json!({"repository": "nas"})),
        ("list_operations", json!({})),
        ("get_operation", json!({"operation": summary(1).id})),
        ("run_plan", json!({"plan": "home"})),
        ("wait_operation", json!({"operation": summary(1).id})),
        ("cancel_operation", json!({"operation": summary(1).id})),
        ("test_repository", json!({"repository": "nas"})),
        ("preview_forget", json!({"repository": "nas"})),
        (
            "preview_import",
            json!({"content": "[repository]\n", "repository": "nas"}),
        ),
        ("export_profile", json!({"plan": "home"})),
        ("propose_setup", json!({"changes": [remove_home()]})),
        ("get_proposal", json!({"proposal": "p"})),
        ("withdraw_proposal", json!({"proposal": "p"})),
    ]
}

/// What each tool answers has every key its output schema requires and,
/// where the schema takes no others, only those it names.
#[tokio::test(start_paused = true)]
async fn every_tools_answer_has_the_keys_its_output_schema_names() {
    let schemas = golden_output_schemas();
    let mut status = connect(told());
    let mut client = proposing(Lists {
        plans: plans(1),
        repositories: vec![repository("nas")],
        snapshots: vec![snapshot(1)],
        page: Some(OperationsPageDto {
            operations: vec![summary(1)],
            next: None,
        }),
        detail: Some(OperationDetailDto {
            operation: summary(1),
            warnings: vec![warning(1)],
        }),
        preview: Some(forget_preview()),
        ..Lists::default()
    });
    let mut checked = Vec::new();
    for (tool, arguments) in every_tool_called() {
        let content = if tool == "status" {
            status.listed(tool, arguments).await
        } else {
            client.listed(tool, arguments).await
        };
        let schema = schemas
            .get(tool)
            .unwrap_or_else(|| panic!("no schema for {tool}"));
        if let Err(why) = keys_conform(&content, schema) {
            panic!("{tool}: {why}");
        }
        checked.push(tool);
    }
    assert_eq!(checked.len(), schemas.len(), "every tool listed is checked");
}

#[tokio::test(start_paused = true)]
async fn run_plan_returns_the_id_of_the_backup_the_backend_queued() {
    let lists = Lists::default();
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    let content = client.listed("run_plan", json!({"plan": "home"})).await;
    assert_eq!(content, json!({"operation": "started"}));
    assert_eq!(*asked.lock().unwrap(), ["run home"]);
}

#[tokio::test(start_paused = true)]
async fn cancel_operation_asks_the_backend_to_cancel_and_names_the_operation() {
    let lists = Lists::default();
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    let id = summary(1).id;
    let content = client
        .listed("cancel_operation", json!({"operation": id}))
        .await;
    assert_eq!(content, json!({"operation": id}));
    assert_eq!(*asked.lock().unwrap(), [format!("cancel {id}")]);
}

#[tokio::test(start_paused = true)]
async fn a_cancel_the_backend_refuses_is_its_refusal_marked_as_an_error() {
    let mut client = connect(Lists {
        refusal: Some(not_cancellable()),
        ..Lists::default()
    });
    let answer = client
        .call("cancel_operation", json!({"operation": summary(1).id}))
        .await;
    assert_eq!(
        refusal(&answer),
        json!({"code": "api-token-scope", "args": {"scope": {"type": "text", "value": "session"}}}),
    );
}

/// A forget preview as the backend tells one, with its selection.
fn forget_preview() -> MaintenancePreviewDto {
    MaintenancePreviewDto::Forget {
        plans: vec![api::ForgetPlanDto {
            plan: "home".to_owned(),
            total: 3,
            remove: 1,
            listed: vec![snapshot(1)],
        }],
        left_alone: api::LeftAloneDto::default(),
        selection: "a-selection".to_owned(),
        warnings: Vec::new(),
    }
}

#[tokio::test(start_paused = true)]
async fn the_checks_ask_the_backend_with_their_arguments_and_return_what_it_told() {
    let lists = Lists {
        preview: Some(forget_preview()),
        ..Lists::default()
    };
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    let tested = client
        .listed("test_repository", json!({"repository": "nas"}))
        .await;
    assert_eq!(
        tested,
        json!({"locations": [{"position": 1, "address": "/srv/nas", "outcome": "used"}]})
    );
    let imported = client
        .listed(
            "preview_import",
            json!({"content": "[repository]\n", "repository": "nas"}),
        )
        .await;
    assert_eq!(
        imported,
        json!({"repository": "nas", "plans": ["home"], "notices": []})
    );
    let exported = client
        .listed("export_profile", json!({"plan": "home"}))
        .await;
    assert_eq!(exported, json!({"text": "[repository]\n", "notices": []}));
    client
        .listed("preview_forget", json!({"repository": "nas"}))
        .await;
    assert_eq!(
        *asked.lock().unwrap(),
        [
            "test nas",
            "import preview of \"[repository]\\n\" as nas",
            "export home",
            "forget preview of nas",
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn preview_forget_leaves_out_the_selection_only_a_forgets_start_takes() {
    let mut client = connect(Lists {
        preview: Some(forget_preview()),
        ..Lists::default()
    });
    let content = client
        .listed("preview_forget", json!({"repository": "nas"}))
        .await;
    assert_eq!(
        content,
        json!({
            "plans": [{
                "plan": "home",
                "total": 3,
                "remove": 1,
                "listed": [serde_json::to_value(snapshot(1)).unwrap()],
            }],
            "left-alone": {
                "orphaned": {"groups": 0, "snapshots": 0},
                "unmanaged": {"groups": 0, "snapshots": 0},
            },
        })
    );
}

#[tokio::test(start_paused = true)]
async fn a_forget_preview_answered_with_another_kind_is_daemon_unreachable() {
    let mut client = connect(Lists {
        preview: Some(MaintenancePreviewDto::Check { read_data: false }),
        ..Lists::default()
    });
    let answer = client
        .call("preview_forget", json!({"repository": "nas"}))
        .await;
    assert_eq!(refusal(&answer), json!({"code": "daemon-unreachable"}));
}

#[tokio::test(start_paused = true)]
async fn the_acting_tools_refuse_an_argument_they_cant_take_naming_it() {
    let lists = Lists::default();
    let asked = Arc::clone(&lists.asked);
    let mut client = connect(lists);
    for (tool, arguments, field, problem) in [
        ("run_plan", json!({}), "plan", "missing"),
        (
            "run_plan",
            json!({"plan": "home", "now": true}),
            "now",
            "unknown-key",
        ),
        (
            "wait_operation",
            json!({"timeout": 5}),
            "operation",
            "missing",
        ),
        (
            "wait_operation",
            json!({"operation": "x", "timeout": -1}),
            "timeout",
            "invalid",
        ),
        ("wait_operation", json!({"id": "x"}), "id", "unknown-key"),
        ("cancel_operation", json!({}), "operation", "missing"),
        ("cancel_operation", json!({"id": "x"}), "id", "unknown-key"),
    ] {
        let answer = client.call(tool, arguments).await;
        assert_eq!(refusal(&answer), invalid(field, problem), "{tool}");
    }
    assert!(asked.lock().unwrap().is_empty(), "{:?}", asked.lock());
}

/// More summary reads than any wait makes: 121 in sixty seconds. A wait
/// that goes on past its end without sleeping never lets paused time move,
/// so the stand-in refuses from then on and the wait ends in an error.
const MOST_READS: u32 = 1000;

/// A stand-in whose one operation is running for its first `running`
/// summary reads and has succeeded from then on; its detail tells what the
/// last summary read told, or is `refused` with that refusal when set. It
/// counts the reads of each, refuses those after [`MOST_READS`] and records
/// any cancel asked of it.
#[derive(Default)]
struct Running {
    running: u32,
    refused: Option<ErrorDto>,
    reads: Arc<Mutex<u32>>,
    details: Arc<Mutex<u32>>,
    cancels: Arc<Mutex<Vec<String>>>,
}

impl Running {
    /// An operation that never finishes.
    fn forever() -> Self {
        Self {
            running: u32::MAX,
            ..Self::default()
        }
    }

    /// The operation's summary, `running` or succeeded.
    fn state(running: bool) -> OperationSummaryDto {
        let mut operation = summary(1);
        if running {
            operation.status = "running".to_owned();
            operation.finished_at = None;
        } else {
            operation.status = "succeeded".to_owned();
        }
        operation
    }
}

#[async_trait]
impl Backend for Running {
    async fn status(&self) -> Result<BackendStatus, ErrorDto> {
        Err(untold())
    }

    async fn plans(&self) -> Result<PlansDto, ErrorDto> {
        Err(untold())
    }

    async fn repositories(&self) -> Result<RepositoriesDto, ErrorDto> {
        Err(untold())
    }

    async fn snapshots(&self, _repository: String) -> Result<SnapshotsDto, ErrorDto> {
        Err(untold())
    }

    async fn history(
        &self,
        _plan: Option<String>,
        _after: Option<String>,
        _limit: u64,
    ) -> Result<OperationsPageDto, ErrorDto> {
        Err(untold())
    }

    async fn operation(&self, _id: String) -> Result<OperationDetailDto, ErrorDto> {
        *self.details.lock().unwrap() += 1;
        if let Some(refusal) = &self.refused {
            return Err(refusal.clone());
        }
        let reads = *self.reads.lock().unwrap();
        Ok(OperationDetailDto {
            operation: Running::state(reads <= self.running),
            warnings: vec![warning(1)],
        })
    }

    async fn summary(&self, _id: String) -> Result<OperationSummaryDto, ErrorDto> {
        let mut reads = self.reads.lock().unwrap();
        if *reads >= MOST_READS {
            return Err(untold());
        }
        let running = *reads < self.running;
        *reads += 1;
        Ok(Running::state(running))
    }

    async fn run(&self, _plan: String) -> Result<String, ErrorDto> {
        Err(untold())
    }

    async fn cancel(&self, id: String) -> Result<(), ErrorDto> {
        self.cancels.lock().unwrap().push(id);
        Ok(())
    }

    async fn test_repository(&self, _repository: String) -> Result<LocationsTestDto, ErrorDto> {
        Err(untold())
    }

    async fn preview_forget(&self, _repository: String) -> Result<MaintenancePreviewDto, ErrorDto> {
        Err(untold())
    }

    async fn preview_import(
        &self,
        _content: String,
        _repository: String,
    ) -> Result<ProfileImportDto, ErrorDto> {
        Err(untold())
    }

    async fn export_profile(&self, _plan: String) -> Result<ProfileExportDto, ErrorDto> {
        Err(untold())
    }
}

impl Client {
    /// Calls `wait_operation` with `timeout`, the call carrying the progress
    /// token `progress` when given, and returns the whole answer.
    async fn wait(&mut self, timeout: Option<u64>, progress: Option<&str>) -> Value {
        let mut meta = current();
        if let Some(token) = progress {
            meta["progressToken"] = json!(token);
        }
        let mut arguments = json!({"operation": summary(1).id});
        if let Some(timeout) = timeout {
            arguments["timeout"] = json!(timeout);
        }
        self.ask(
            "tools/call",
            json!({"name": "wait_operation", "arguments": arguments, "_meta": meta}),
        )
        .await
    }

    /// The progress notifications read so far that answered no request.
    fn progress(&self) -> Vec<&Value> {
        self.others
            .iter()
            .filter(|message| message["method"] == "notifications/progress")
            .collect()
    }
}

/// The wait polls the operation's summary, bounded however many warnings
/// it recorded, and reads its detail, warnings and all, once at the end.
#[tokio::test(start_paused = true)]
async fn wait_operation_reads_the_summary_every_half_second_until_it_finishes_then_the_detail() {
    let running = Running {
        running: 3,
        ..Running::default()
    };
    let (reads, details) = (Arc::clone(&running.reads), Arc::clone(&running.details));
    let mut client = connect(running);
    let began = tokio::time::Instant::now();
    let answer = client.wait(None, None).await;
    assert_eq!(began.elapsed(), Duration::from_millis(1500));
    assert_eq!(*reads.lock().unwrap(), 4);
    assert_eq!(*details.lock().unwrap(), 1);
    let content = &answer["result"]["structuredContent"];
    assert_eq!(content["finished"], true, "{answer}");
    assert_eq!(content["operation"]["status"], "succeeded", "{answer}");
    assert_eq!(content["warnings"].as_array().map(Vec::len), Some(1));
    assert_eq!(content["unlisted-warnings"], 0);
}

#[tokio::test(start_paused = true)]
async fn a_wait_that_times_out_returns_the_operation_unfinished() {
    let running = Running::forever();
    let (reads, details) = (Arc::clone(&running.reads), Arc::clone(&running.details));
    let mut client = connect(running);
    let began = tokio::time::Instant::now();
    let answer = client.wait(Some(2), None).await;
    assert_eq!(began.elapsed(), Duration::from_secs(2));
    // At 0, 0.5, 1, 1.5 and 2 seconds.
    assert_eq!(*reads.lock().unwrap(), 5);
    assert_eq!(*details.lock().unwrap(), 1);
    assert_eq!(answer["result"]["isError"], false, "{answer}");
    let content = &answer["result"]["structuredContent"];
    assert_eq!(content["finished"], false, "{answer}");
    assert_eq!(content["operation"]["status"], "running", "{answer}");
}

/// An operation with too many warnings to read whole still tells how it
/// ended: the wait answers with the summary it just read, no warnings
/// listed, rather than refusing.
#[tokio::test(start_paused = true)]
async fn a_wait_whose_detail_is_too_large_to_read_returns_the_summary_it_read() {
    let too_large = ErrorDto::new(
        ErrorCode::DaemonAnswerTooLarge,
        [("limit", api::MsgArg::Bytes(8 * 1024 * 1024))],
        None,
    );
    for (running, finished, status) in [(1, true, "succeeded"), (u32::MAX, false, "running")] {
        let running = Running {
            running,
            refused: Some(too_large.clone()),
            ..Running::default()
        };
        let details = Arc::clone(&running.details);
        let mut client = connect(running);
        let answer = client.wait(Some(2), None).await;
        assert_eq!(*details.lock().unwrap(), 1);
        assert_eq!(answer["result"]["isError"], false, "{answer}");
        let content = &answer["result"]["structuredContent"];
        assert_eq!(content["finished"], finished, "{answer}");
        assert_eq!(content["operation"]["status"], status, "{answer}");
        assert_eq!(content["warnings"], json!([]), "{answer}");
        assert_eq!(content.get("unlisted-warnings"), None, "{answer}");
    }
}

/// Only a detail too large to read falls back to the summary: any other
/// refusal of it is handed on.
#[tokio::test(start_paused = true)]
async fn a_wait_whose_detail_is_refused_otherwise_is_that_refusal() {
    let unreachable = ErrorDto::new(ErrorCode::DaemonUnreachable, [], None);
    let mut client = connect(Running {
        running: 1,
        refused: Some(unreachable.clone()),
        ..Running::default()
    });
    let answer = client.wait(None, None).await;
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    assert_eq!(
        refusal(&answer),
        serde_json::to_value(&unreachable).unwrap()
    );
}

#[tokio::test(start_paused = true)]
async fn a_wait_of_no_seconds_reads_the_operation_once() {
    let running = Running::forever();
    let (reads, details) = (Arc::clone(&running.reads), Arc::clone(&running.details));
    let mut client = connect(running);
    let began = tokio::time::Instant::now();
    let answer = client.wait(Some(0), None).await;
    assert_eq!(began.elapsed(), Duration::ZERO);
    assert_eq!(*reads.lock().unwrap(), 1);
    assert_eq!(*details.lock().unwrap(), 1);
    assert_eq!(answer["result"]["structuredContent"]["finished"], false);
}

#[test]
fn a_wait_lasts_sixty_seconds_unless_told_and_never_over_six_hundred() {
    assert_eq!(actions::wait_length(None), Duration::from_secs(60));
    assert_eq!(actions::wait_length(Some(5)), Duration::from_secs(5));
    assert_eq!(actions::wait_length(Some(600)), Duration::from_secs(600));
    assert_eq!(actions::wait_length(Some(601)), Duration::from_secs(600));
    assert_eq!(
        actions::wait_length(Some(u64::MAX)),
        Duration::from_secs(600)
    );
}

#[tokio::test(start_paused = true)]
async fn a_wait_sends_the_seconds_waited_as_progress_when_the_call_carries_a_token() {
    let mut client = connect(Running::forever());
    client.wait(Some(1), Some("wait-1")).await;
    let sent: Vec<Value> = client
        .progress()
        .iter()
        .map(|message| message["params"].clone())
        .collect();
    let expected = |seconds: f64| json!({"progressToken": "wait-1", "progress": seconds, "total": 1.0, "message": "running"});
    assert_eq!(sent, [expected(0.0), expected(0.5)]);
}

#[tokio::test(start_paused = true)]
async fn a_wait_sends_no_progress_when_the_call_carries_no_token() {
    let mut client = connect(Running::forever());
    client.wait(Some(2), None).await;
    assert_eq!(client.progress(), Vec::<&Value>::new());
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_wait_stops_the_wait_and_never_the_operation() {
    let running = Running::forever();
    let (reads, details, cancels) = (
        Arc::clone(&running.reads),
        Arc::clone(&running.details),
        Arc::clone(&running.cancels),
    );
    let mut client = connect(running);
    client.next += 1;
    let waiting = client.next;
    let mut meta = current();
    meta["progressToken"] = json!("wait-1");
    client
        .send(&json!({
            "jsonrpc": "2.0",
            "id": waiting,
            "method": "tools/call",
            "params": {
                "name": "wait_operation",
                "arguments": {"operation": summary(1).id, "timeout": 60},
                "_meta": meta,
            },
        }))
        .await;
    // The first progress notification: the wait has read the operation.
    let first = client.next_message().await;
    assert_eq!(first["method"], "notifications/progress", "{first}");
    client
        .send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": waiting, "reason": "the user stopped waiting"},
        }))
        .await;
    // Long enough for a wait that went on to read the operation many times.
    tokio::time::sleep(Duration::from_secs(10)).await;
    let after = *reads.lock().unwrap();
    assert!(
        after <= 2,
        "the wait went on after its call was cancelled: {after} reads"
    );
    assert!(cancels.lock().unwrap().is_empty(), "{cancels:?}");
    // Its answer is dropped, so the detail isn't read for it.
    assert_eq!(*details.lock().unwrap(), 0);
    // The session goes on, and the cancelled call is never answered.
    let answer = client.ask_current("tools/list", json!({})).await;
    assert!(answer["result"]["tools"].is_array(), "{answer}");
    assert!(
        client.others.iter().all(|message| message["id"] != waiting),
        "the cancelled call was answered: {:?}",
        client.others
    );
}

// One-shot calls (§4.6): `ladderback mcp call`, through the dispatch
// `tools/call` uses.

fn words(given: &[&str]) -> Vec<String> {
    given.iter().map(|&word| word.to_owned()).collect()
}

fn object(value: Value) -> JsonObject {
    match value {
        Value::Object(object) => object,
        other => panic!("not an object: {other}"),
    }
}

#[test]
fn words_are_read_as_the_tools_input_schema_types_them() {
    let call = Call::of_words(
        "list_operations",
        &words(&["limit=10", "plan=123", "cursor=a=b"]),
    )
    .unwrap();
    assert_eq!(call.tool(), "list_operations");
    assert_eq!(
        Value::Object(call.arguments().clone()),
        json!({"limit": 10, "plan": "123", "cursor": "a=b"})
    );
}

#[test]
fn a_word_for_an_integer_that_isnt_one_stays_text_for_the_tool_to_refuse() {
    for (word, value) in [
        ("limit=many", json!("many")),
        ("limit=1.5", json!("1.5")),
        ("limit=", json!("")),
        ("limit=-3", json!(-3)),
        ("limit=18446744073709551615", json!(u64::MAX)),
    ] {
        let call = Call::of_words("list_plans", &words(&[word])).unwrap();
        assert_eq!(call.arguments().get("limit"), Some(&value), "{word}");
    }
}

#[test]
fn a_value_is_read_as_the_type_the_schema_gives_its_argument_and_as_text_otherwise() {
    let schema = object(json!({
        "type": "object",
        "properties": {
            "count": {"type": "integer"},
            "verbose": {"type": "boolean"},
            "name": {"type": "string"},
        },
    }));
    for (name, value, typed_as) in [
        ("count", "7", json!(7)),
        ("count", "seven", json!("seven")),
        ("verbose", "true", json!(true)),
        ("verbose", "false", json!(false)),
        ("verbose", "yes", json!("yes")),
        ("name", "7", json!("7")),
        ("name", "true", json!("true")),
        ("other", "7", json!("7")),
    ] {
        assert_eq!(
            one_shot::typed(&schema, name, value),
            typed_as,
            "{name}={value}"
        );
    }
}

#[test]
fn a_word_that_isnt_a_pair_is_refused_by_its_position() {
    assert_eq!(
        Call::of_words("list_plans", &words(&["limit=1", "cursor"])),
        Err(CallRefused::NotAPair(2))
    );
    assert_eq!(
        Call::of_words("list_plans", &words(&["=1"])),
        Err(CallRefused::NotAPair(1))
    );
}

#[test]
fn an_argument_given_twice_is_refused_naming_it() {
    assert_eq!(
        Call::of_words("list_plans", &words(&["limit=1", "cursor=a", "limit=2"])),
        Err(CallRefused::Repeated("limit".to_owned()))
    );
}

#[test]
fn no_words_call_the_tool_with_no_arguments() {
    let call = Call::of_words("status", &[]).unwrap();
    assert_eq!(call.tool(), "status");
    assert!(call.arguments().is_empty());
}

#[test]
fn a_tool_that_isnt_there_is_refused_before_its_arguments_are_read() {
    assert_eq!(
        Call::of_words("restore", &words(&["x"])),
        Err(CallRefused::NoTool)
    );
    assert_eq!(Call::of_object("restore", "[1]"), Err(CallRefused::NoTool));
    assert_eq!(
        Call::of_line(r#"{"tool": "restore"}"#),
        Err(CallRefused::NoTool)
    );
}

#[test]
fn arguments_as_json_are_one_object_taken_as_given() {
    let call = Call::of_object("list_plans", r#"{"limit": "5", "cursor": "a"}"#).unwrap();
    assert_eq!(call.tool(), "list_plans");
    assert_eq!(
        Value::Object(call.arguments().clone()),
        json!({"limit": "5", "cursor": "a"})
    );
    for given in ["[1]", "5", "\"limit\"", "null", "{", ""] {
        assert_eq!(
            Call::of_object("list_plans", given),
            Err(CallRefused::NotAnObject),
            "{given}"
        );
    }
}

#[test]
fn a_script_line_is_a_tool_and_its_arguments() {
    let call = Call::of_line(r#"{"tool": "list_plans", "arguments": {"limit": 2}}"#).unwrap();
    assert_eq!(call.tool(), "list_plans");
    assert_eq!(Value::Object(call.arguments().clone()), json!({"limit": 2}));
    let call = Call::of_line(r#"{"tool": "status"}"#).unwrap();
    assert_eq!(call.tool(), "status");
    assert!(call.arguments().is_empty());
    for line in [
        r#"{"tool": "status", "args": {}}"#,
        r#"{"tool": "list_plans", "arguments": [2]}"#,
        r#"{"arguments": {}}"#,
        r#"["status"]"#,
        "status",
    ] {
        assert_eq!(Call::of_line(line), Err(CallRefused::NotACall), "{line}");
    }
}

#[test]
fn every_tool_listed_can_be_called_by_its_name() {
    for tool in tools() {
        let call = Call::of_words(&tool.name, &[]).unwrap();
        assert_eq!(call.tool(), tool.name, "{}", tool.name);
    }
}

#[test]
fn a_calls_line_is_its_tool_with_its_result_or_its_refusal() {
    let done = Answer::Done(json!({"plans": []}));
    assert!(done.is_ok());
    assert_eq!(
        serde_json::to_value(done.outcome("list_plans")).unwrap(),
        json!({"tool": "list_plans", "ok": true, "result": {"plans": []}})
    );
    let refused = Answer::Refused(ErrorDto::new(ErrorCode::SignInRequired, [], None));
    assert!(!refused.is_ok());
    assert_eq!(
        serde_json::to_value(refused.outcome("status")).unwrap(),
        json!({"tool": "status", "ok": false, "error": {"code": "sign-in-required"}})
    );
    let unwritten = Answer::Unwritten("key must be a string".to_owned());
    assert!(!unwritten.is_ok());
    assert_eq!(
        serde_json::to_value(unwritten.outcome("status")).unwrap(),
        json!({"tool": "status", "ok": false, "error": "key must be a string"})
    );
}

/// A client of a session, and a server for one-shot calls, both connect
/// mode's over `lists`.
fn both_over(lists: Lists) -> (Client, Server) {
    let lists = Arc::new(lists);
    let server = Server::new(Arc::clone(&lists) as Arc<dyn Backend>, build(), english())
        .proposing(Arc::clone(&lists) as Arc<dyn Proposals>);
    let client = serve(
        Server::new(Arc::clone(&lists) as Arc<dyn Backend>, build(), english()).proposing(lists),
    );
    (client, server)
}

#[tokio::test(start_paused = true)]
async fn a_one_shot_call_answers_as_a_tools_call_does() {
    let (mut client, server) = both_over(Lists {
        plans: plans(3),
        repositories: vec![repository("nas")],
        snapshots: vec![snapshot(1)],
        detail: Some(OperationDetailDto {
            operation: summary(1),
            warnings: vec![warning(1)],
        }),
        preview: Some(forget_preview()),
        ..Lists::default()
    });
    let id = summary(1).id;
    for (tool, arguments) in [
        ("status", json!({})),
        ("status", json!({"plan": "home"})),
        ("list_plans", json!({"limit": 2})),
        ("list_plans", json!({"limit": "many"})),
        ("list_plans", json!({"cursor": "nowhere"})),
        ("list_repositories", json!({})),
        ("list_snapshots", json!({"repository": "nas"})),
        ("list_operations", json!({"plan": "home"})),
        ("get_operation", json!({"operation": id})),
        ("run_plan", json!({"plan": "home"})),
        ("run_plan", json!({})),
        ("wait_operation", json!({"operation": id, "timeout": 0})),
        ("cancel_operation", json!({"operation": id})),
        ("test_repository", json!({"repository": "nas"})),
        ("preview_forget", json!({"repository": "nas"})),
        (
            "preview_import",
            json!({"content": "[repository]\n", "repository": "nas"}),
        ),
        ("export_profile", json!({"plan": "home"})),
        ("propose_setup", json!({"changes": [remove_home()]})),
        ("propose_setup", json!({"changes": "all"})),
        ("get_proposal", json!({"proposal": "p"})),
        ("withdraw_proposal", json!({})),
    ] {
        let answer = client.call(tool, arguments.clone()).await;
        let call =
            Call::of_line(&json!({"tool": tool, "arguments": arguments}).to_string()).unwrap();
        match server.call(call).await {
            Answer::Done(result) => {
                assert_eq!(answer["result"]["isError"], false, "{tool}: {answer}");
                assert_eq!(answer["result"]["structuredContent"], result, "{tool}");
            }
            Answer::Refused(error) => {
                assert_eq!(
                    refusal(&answer),
                    serde_json::to_value(&error).unwrap(),
                    "{tool}"
                );
                assert_eq!(said(&answer), server.sentence(&error), "{tool}");
            }
            Answer::Unwritten(why) => panic!("{tool}: unwritten: {why}"),
        }
    }
}

/// A one-shot wait has no client to cancel it or to tell its progress: it
/// waits its length, then answers what it last read.
#[tokio::test(start_paused = true)]
async fn a_one_shot_wait_waits_its_length_with_no_client() {
    let mut running = summary(1);
    running.status = "running".to_owned();
    let id = running.id.clone();
    let lists = Lists {
        detail: Some(OperationDetailDto {
            operation: running,
            warnings: Vec::new(),
        }),
        ..Lists::default()
    };
    let server = Server::new(Arc::new(lists), build(), english());
    let call = Call::of_line(
        &json!({"tool": "wait_operation", "arguments": {"operation": id, "timeout": 3}})
            .to_string(),
    )
    .unwrap();
    let began = tokio::time::Instant::now();
    let Answer::Done(waited) = server.call(call).await else {
        panic!("the wait was refused")
    };
    assert_eq!(began.elapsed(), Duration::from_secs(3));
    assert_eq!(waited["finished"], false, "{waited}");
    assert_eq!(waited["operation"]["status"], "running", "{waited}");
}

/// A change that removes the plan `home`, as a call gives it.
fn remove_home() -> Value {
    json!({"kind": "remove-plan", "plan": "home"})
}

/// A session of its own offers no proposal tool: none is listed, and one
/// called names no tool there, as a name no tool has does (§4.6).
#[tokio::test(start_paused = true)]
async fn a_server_of_its_own_offers_no_proposal_tool() {
    let mut connected = proposing(Lists::default());
    let every = connected.ask_current("tools/list", json!({})).await["result"]["tools"].clone();
    let mut client = connect(Lists::default());
    let answer = client.ask_current("tools/list", json!({})).await;
    let proposal_tools = ["propose_setup", "get_proposal", "withdraw_proposal"];
    let expected: Vec<&Value> = every
        .as_array()
        .expect("a list of tools")
        .iter()
        .filter(|tool| !proposal_tools.contains(&tool["name"].as_str().unwrap_or_default()))
        .collect();
    let listed: Vec<&Value> = answer["result"]["tools"]
        .as_array()
        .expect("a list of tools")
        .iter()
        .collect();
    assert_eq!(listed, expected);
    assert_eq!(listed.len(), 13);
    for tool in proposal_tools {
        let answer = client.call(tool, json!({"proposal": "p"})).await;
        assert_eq!(answer["error"]["code"], -32602, "{tool}: {answer}");
        assert_eq!(
            answer["error"]["message"],
            format!("no tool is named {tool:?}"),
            "{tool}: {answer}"
        );
    }
}

/// No tool approves or rejects a proposal, or edits the configuration: a
/// person does, in the web UI (decision 89, point 7).
#[tokio::test(start_paused = true)]
async fn no_tool_approves_rejects_or_edits() {
    let mut client = proposing(Lists::default());
    let answer = client.ask_current("tools/list", json!({})).await;
    for tool in answer["result"]["tools"]
        .as_array()
        .expect("a list of tools")
    {
        let name = tool["name"].as_str().expect("a name");
        for word in ["approve", "reject", "apply", "edit", "config"] {
            assert!(!name.contains(word), "{name}");
        }
    }
    for tool in ["approve_proposal", "reject_proposal"] {
        let answer = client.call(tool, json!({"proposal": "p"})).await;
        assert_eq!(answer["error"]["code"], -32602, "{tool}: {answer}");
    }
}

#[test]
fn the_servers_offer_is_told_by_whether_it_proposes() {
    assert_eq!(
        Server::new(Arc::new(Lists::default()), build(), english()).offer(),
        Offer::InProcess
    );
    assert_eq!(proposing_server(Lists::default()).offer(), Offer::Connect);
}

/// A one-shot call of a proposal tool is read, then refused where it isn't
/// offered; every other tool is offered in both modes.
#[test]
fn only_connect_mode_offers_the_proposal_tools_to_a_one_shot_call() {
    for tool in tools() {
        let call = Call::of_words(&tool.name, &[]).unwrap();
        let proposes = matches!(
            tool.name.as_ref(),
            "propose_setup" | "get_proposal" | "withdraw_proposal"
        );
        assert!(call.offered_in(Offer::Connect), "{}", tool.name);
        assert_eq!(
            call.offered_in(Offer::InProcess),
            !proposes,
            "{}",
            tool.name
        );
    }
}

/// A server of its own given a proposal tool's call all the same answers
/// that it has no such tool, and asks nothing.
#[tokio::test(start_paused = true)]
async fn a_one_shot_proposal_call_on_a_server_of_its_own_is_unwritten() {
    let lists = Lists::default();
    let asked = Arc::clone(&lists.asked);
    let server = Server::new(Arc::new(lists), build(), english());
    let call = Call::of_words("get_proposal", &words(&["proposal=p"])).unwrap();
    let Answer::Unwritten(why) = server.call(call).await else {
        panic!("a proposal tool was called on a server of its own")
    };
    assert_eq!(why, "no tool is named \"get_proposal\"");
    assert!(asked.lock().unwrap().is_empty());
}

/// `propose_setup` hands the changes and the rationale to the daemon as the
/// proposal route takes them, and returns the proposal it kept, warnings
/// and all; `get_proposal` and `withdraw_proposal` name the proposal.
#[tokio::test(start_paused = true)]
async fn the_proposal_tools_ask_the_daemon_and_return_its_proposal() {
    let lists = Lists::default();
    let asked = Arc::clone(&lists.asked);
    let mut client = proposing(lists);
    let kept = serde_json::to_value(a_proposal()).unwrap();
    let add = json!({"kind": "add-plan", "plan": {"id": "nightly", "repository": "nas", "sources": ["/home"]}});
    let given = json!({"changes": [add, remove_home()], "rationale": "Every night."});
    assert_eq!(client.listed("propose_setup", given.clone()).await, kept);
    assert_eq!(
        client
            .listed("propose_setup", json!({"changes": [remove_home()]}))
            .await,
        kept
    );
    let id = "0197a000-0000-7000-8000-000000000001";
    assert_eq!(
        client.listed("get_proposal", json!({"proposal": id})).await,
        kept
    );
    assert_eq!(
        client
            .listed("withdraw_proposal", json!({"proposal": id}))
            .await,
        kept
    );
    let no_rationale = json!({"changes": [remove_home()], "rationale": ""});
    assert_eq!(
        *asked.lock().unwrap(),
        [
            format!("propose {given}"),
            format!("propose {no_rationale}"),
            format!("proposal {id}"),
            format!("withdraw {id}"),
        ]
    );
}

/// A call the proposal tools can't read is `invalid-arguments` naming the
/// argument, never the value given, and asks the daemon nothing.
#[tokio::test(start_paused = true)]
async fn a_proposal_tool_refuses_an_argument_it_cant_take_naming_it() {
    let lists = Lists::default();
    let asked = Arc::clone(&lists.asked);
    let mut client = proposing(lists);
    for (tool, arguments, field, problem) in [
        ("propose_setup", json!({}), "changes", "missing"),
        (
            "propose_setup",
            json!({"changes": "canary"}),
            "changes",
            "invalid",
        ),
        (
            "propose_setup",
            json!({"changes": [{"kind": "approve", "plan": "canary"}]}),
            "changes",
            "invalid",
        ),
        (
            "propose_setup",
            json!({"changes": [], "rationale": 1}),
            "rationale",
            "invalid",
        ),
        (
            "propose_setup",
            json!({"changes": [], "approve": true}),
            "approve",
            "unknown-key",
        ),
        ("get_proposal", json!({}), "proposal", "missing"),
        (
            "withdraw_proposal",
            json!({"proposal": 7}),
            "proposal",
            "invalid",
        ),
        (
            "withdraw_proposal",
            json!({"proposal": "p", "confirmation": "canary"}),
            "confirmation",
            "unknown-key",
        ),
    ] {
        let answer = client.call(tool, arguments).await;
        assert_eq!(refusal(&answer), invalid(field, problem), "{tool}");
        assert!(!answer.to_string().contains("canary"), "{answer}");
    }
    assert!(asked.lock().unwrap().is_empty(), "{:?}", asked.lock());
}
