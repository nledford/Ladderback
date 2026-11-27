//! The user guide's page on AI agents (#1188) lists the tools `ladderback mcp`
//! offers as the server lists them: each tool's name and description, the
//! permissions that open it, and whether a session of its own offers it. The
//! list is the server's, never typed by hand, so a tool added, renamed,
//! described anew or opened by another permission fails this test until the
//! page shows it.
//!
//! The server lists the tools to a client of its own, connected to a stand-in
//! daemon whose answer to `GET /api/v1/permissions` the test chooses, as
//! connect mode reads a token's permissions (decision 127).

use std::sync::{Arc, Mutex, PoisonError};

use api::{BuildDto, MsgArg, PermissionsDto, PlanDto};
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use domain::access::permissions::Permission;
use l10n::{Localizer, msg};
use mcp::{Backend, Connect, DaemonUrl, PlanTables, Proposals, Server};
use serde_json::{Map, Value, json};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

/// The page, `docs/guide/<PAGE>.md`.
const PAGE: &str = "ai-agents";
/// The line before the list on the page.
const START: &str = "<!-- The tools, as the server lists them: crates/adapters/mcp/tests/guide.rs checks this list. -->";
/// The line after it.
const END: &str = "<!-- The end of the tools. -->";

#[tokio::test]
async fn the_guide_lists_the_tools_the_server_offers() {
    let list = tools_list().await;
    let path = format!(
        "{}/../../../docs/guide/{PAGE}.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let page = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{path} is readable: {error}"));
    let listed = page
        .split_once(&format!("{START}\n"))
        .and_then(|(_, after)| after.split_once(&format!("\n{END}")))
        .map(|(listed, _)| listed);
    assert!(
        listed == Some(list.as_str()),
        "the tools on {path} aren't the server's; write this between its lines\n\
         {START}\nand\n{END}\n\n{list}"
    );
}

/// The permission as the web UI names it, from the catalog, as the guide
/// names it too.
fn label(permission: Permission) -> String {
    let name = Localizer::new(&[]).message(
        msg::UI_PERMISSION,
        &[("permission", MsgArg::Text(permission.as_str().to_owned()))],
    );
    assert!(name.problems().is_empty(), "{:?}", name.problems());
    name.into_text()
}

/// The guide's list of the tools, after the blank line that starts it: an
/// item for each tool, in the order the server lists them, narrow enough for
/// a phone, as a table of the descriptions isn't.
async fn tools_list() -> String {
    let permissions = Arc::new(Mutex::new(granted(None)));
    let daemon = stand_in_daemon(Arc::clone(&permissions)).await;
    let connect = Arc::new(
        Connect::new(DaemonUrl::parse(&daemon).expect("a daemon URL"))
            .expect("a client of the daemon"),
    );
    let build = BuildDto {
        version: "0.0.0".to_owned(),
        commit: None,
        dirty: false,
        release: true,
    };
    // Connect mode's server, which offers the proposal tools, and one that
    // offers the tools of a session of its own (headless or scratch).
    let mut connected = Client::of(
        Server::new(
            Arc::clone(&connect) as Arc<dyn Backend>,
            build.clone(),
            Localizer::new(&[]),
        )
        .proposing(
            Arc::clone(&connect) as Arc<dyn Proposals>,
            Arc::new(NoTables),
        ),
    );
    let mut own_session = Client::of(Server::new(connect, build, Localizer::new(&[])));

    let tools = connected.tools().await;
    let in_own_session = names(&own_session.tools().await);
    // A tool needs each permission without which the server doesn't list it.
    let mut needs: Vec<Vec<String>> = vec![Vec::new(); tools.len()];
    for permission in Permission::ALL {
        *permissions.lock().unwrap_or_else(PoisonError::into_inner) = granted(Some(permission));
        let opened = names(&connected.tools().await);
        for (tool, needed) in tools.iter().zip(needs.iter_mut()) {
            if !opened.contains(&text(tool, "name")) {
                needed.push(label(permission));
            }
        }
    }

    let mut list = String::from("\n");
    for (tool, needed) in tools.iter().zip(needs) {
        let name = text(tool, "name");
        let where_offered = if in_own_session.contains(&name) {
            ""
        } else {
            "; running Ladderback only"
        };
        let description = text(tool, "description").replace('\n', " ");
        list.push_str(&format!(
            "- **`{name}`** ({}{where_offered}). {description}\n",
            needed.join(" and ")
        ));
    }
    list
}

/// Every permission at its level, but `without`, as the daemon answers it.
fn granted(without: Option<Permission>) -> PermissionsDto {
    let levels: Map<String, Value> = Permission::ALL
        .into_iter()
        .filter(|permission| Some(*permission) != without)
        .map(|permission| {
            (
                permission.as_str().to_owned(),
                json!(permission.level().as_str()),
            )
        })
        .collect();
    serde_json::from_value(Value::Object(levels)).expect("permissions the wire carries")
}

/// A daemon on loopback that answers `GET /api/v1/permissions` with what
/// `permissions` holds at the time, and its address.
async fn stand_in_daemon(permissions: Arc<Mutex<PermissionsDto>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port for the daemon");
    let address = listener.local_addr().expect("an address");
    let routes = Router::new()
        .route(
            "/api/v1/permissions",
            get(
                |State(permissions): State<Arc<Mutex<PermissionsDto>>>| async move {
                    axum::Json(*permissions.lock().unwrap_or_else(PoisonError::into_inner))
                },
            ),
        )
        .with_state(permissions);
    tokio::spawn(async move { axum::serve(listener, routes).await });
    format!("http://{address}")
}

/// A reader of plan tables that reads none: listing the tools reads no plan.
struct NoTables;

impl PlanTables for NoTables {
    fn read(&self, _text: &str) -> Option<PlanDto> {
        None
    }
}

/// The names of `tools`.
fn names(tools: &[Value]) -> Vec<String> {
    tools.iter().map(|tool| text(tool, "name")).collect()
}

/// `field` of `tool`, which the server gives as text.
fn text(tool: &Value, field: &str) -> String {
    tool.get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the tool's {field} is text: {tool}"))
        .to_owned()
}

/// The client's end of one session, of the current protocol revision, which
/// asks without a handshake.
struct Client {
    input: WriteHalf<DuplexStream>,
    output: Lines<BufReader<ReadHalf<DuplexStream>>>,
    next: u64,
}

impl Client {
    fn of(server: Server) -> Self {
        let (client, server_end) = tokio::io::duplex(1024 * 1024);
        let (server_input, server_output) = tokio::io::split(server_end);
        tokio::spawn(server.serve(server_input, server_output));
        let (output, input) = tokio::io::split(client);
        Self {
            input,
            output: BufReader::new(output).lines(),
            next: 0,
        }
    }

    /// The tools `tools/list` gives now.
    async fn tools(&mut self) -> Vec<Value> {
        self.next += 1;
        let id = self.next;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": {"_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            }},
        });
        let mut line = request.to_string();
        line.push('\n');
        self.input
            .write_all(line.as_bytes())
            .await
            .expect("the request sent");
        loop {
            let line = self
                .output
                .next_line()
                .await
                .expect("the server's output read")
                .expect("the session still open");
            let message: Value = serde_json::from_str(&line).expect("a JSON message");
            if message.get("id") == Some(&json!(id)) {
                return message
                    .get("result")
                    .and_then(|result| result.get("tools"))
                    .and_then(Value::as_array)
                    .unwrap_or_else(|| panic!("a list of tools: {message}"))
                    .clone();
            }
        }
    }
}
