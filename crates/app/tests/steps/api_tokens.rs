//! Steps for the untagged scenarios of `features/security/api_tokens.feature`:
//! the API token use cases over the sign-in scenarios' `Access`, an in-memory
//! token store and the world's fake clock.

use std::net::IpAddr;

use app::access::{ApiTokenGrant, ApiTokenRefused, ApiTokensRefused, CreatedApiToken};
use app::ports::Clock;
use cucumber::{given, then, when};
use domain::access::api_token::{
    ApiToken, ApiTokenLabel, ApiTokenProblem, PresentedApiToken, Scope, Scopes,
};

use super::Agent;

/// What the API token scenarios keep between steps.
#[derive(Default)]
pub struct ApiTokensWorld {
    /// The token created last, with its text.
    created: Option<CreatedApiToken>,
    /// What the last creation came to, when it was refused.
    refused: Option<Refusal>,
    /// What the last presented token came to.
    checked: Option<Result<ApiTokenGrant, ApiTokenRefused>>,
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Problem(ApiTokenProblem),
    Store(ApiTokensRefused),
}

impl std::fmt::Debug for ApiTokensWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiTokensWorld")
            .field("refused", &self.refused)
            .field("checked", &self.checked)
            .finish_non_exhaustive()
    }
}

fn scopes_of(text: &str) -> Vec<Scope> {
    text.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| Scope::parse(name).expect("a scope"))
        .collect()
}

fn created(agent: &Agent) -> &CreatedApiToken {
    agent
        .api_tokens
        .created
        .as_ref()
        .expect("the admin created a token")
}

async fn create(agent: &mut Agent, label: String, scopes: Vec<Scope>) {
    agent.api_tokens.refused = None;
    let label = match ApiTokenLabel::new(label) {
        Ok(label) => label,
        Err(problem) => {
            agent.api_tokens.refused = Some(Refusal::Problem(problem));
            return;
        }
    };
    let scopes = match Scopes::new(scopes) {
        Ok(scopes) => scopes,
        Err(problem) => {
            agent.api_tokens.refused = Some(Refusal::Problem(problem));
            return;
        }
    };
    match agent.sign_in.access().create_api_token(label, scopes).await {
        Ok(token) => agent.api_tokens.created = Some(token),
        Err(refused) => agent.api_tokens.refused = Some(Refusal::Store(refused)),
    }
}

async fn listed(agent: &Agent) -> Vec<ApiToken> {
    agent
        .sign_in
        .access()
        .api_tokens()
        .await
        .expect("the tokens are listed")
}

async fn present(agent: &mut Agent, text: &str, from: &str) {
    let address: IpAddr = from.parse().expect("an IP address");
    let presented = PresentedApiToken::parse(text);
    let checked = agent
        .sign_in
        .access()
        .check_api_token(address, presented.as_ref())
        .await;
    agent.api_tokens.checked = Some(checked);
}

#[when(expr = "the admin creates an API token {string} with the scopes {string}")]
async fn creates(agent: &mut Agent, label: String, scopes: String) {
    create(agent, label, scopes_of(&scopes)).await;
}

#[given(expr = "the admin has created an API token {string} with the scopes {string}")]
async fn has_created(agent: &mut Agent, label: String, scopes: String) {
    create(agent, label, scopes_of(&scopes)).await;
    assert_eq!(agent.api_tokens.refused, None);
}

#[when(
    expr = "the admin creates an API token with a label of {int} characters and the scopes {string}"
)]
async fn creates_long(agent: &mut Agent, chars: usize, scopes: String) {
    create(agent, "a".repeat(chars), scopes_of(&scopes)).await;
}

#[when(expr = "the admin creates an API token {string} with no scopes")]
async fn creates_without_scopes(agent: &mut Agent, label: String) {
    create(agent, label, Vec::new()).await;
}

#[given(expr = "the admin has created {int} API tokens")]
async fn has_created_many(agent: &mut Agent, count: usize) {
    for index in 0..count {
        create(agent, format!("demo-{index}"), vec![Scope::Read]).await;
        assert_eq!(agent.api_tokens.refused, None, "token {index}");
    }
}

#[then(expr = "the token's secret starts with {string}")]
fn secret_starts(agent: &mut Agent, prefix: String) {
    assert!(created(agent).text.expose().starts_with(&prefix));
}

#[then(expr = "the API tokens are listed as {string} with the scopes {string}, never used")]
async fn listed_as(agent: &mut Agent, label: String, scopes: String) {
    let tokens = listed(agent).await;
    let [token] = tokens.as_slice() else {
        panic!("one token, not {}", tokens.len());
    };
    assert_eq!(token.label.as_str(), label);
    assert_eq!(token.scopes.iter().collect::<Vec<_>>(), scopes_of(&scopes));
    assert_eq!(token.last_used_at, None);
    assert_eq!(token.id, created(agent).token.id);
}

#[then("no listed API token holds the token's secret")]
async fn listing_hides_secret(agent: &mut Agent) {
    let text = created(agent).text.expose().to_owned();
    let secret = text.rsplit('_').next().expect("a secret");
    let shown = format!("{:?}", listed(agent).await);
    assert!(!shown.contains(secret), "{shown}");
}

#[then("the token is refused as having no label of 1 to 64 characters")]
fn refused_label(agent: &mut Agent) {
    assert_eq!(
        agent.api_tokens.refused,
        Some(Refusal::Problem(ApiTokenProblem::Label))
    );
}

#[then("the token is refused as having no scope")]
fn refused_scope(agent: &mut Agent) {
    assert_eq!(
        agent.api_tokens.refused,
        Some(Refusal::Problem(ApiTokenProblem::NoScope))
    );
}

#[then(expr = "the token is refused as one over the {int} kept")]
async fn refused_full(agent: &mut Agent, kept: usize) {
    assert_eq!(
        agent.api_tokens.refused,
        Some(Refusal::Store(ApiTokensRefused::Full))
    );
    assert_eq!(listed(agent).await.len(), kept);
}

#[when(expr = "a program presents the token from {word}")]
async fn presents(agent: &mut Agent, from: String) {
    let text = created(agent).text.expose().to_owned();
    present(agent, &text, &from).await;
}

#[when(expr = "a program presents the token with a wrong secret from {word}")]
async fn presents_wrong(agent: &mut Agent, from: String) {
    let text = created(agent).text.expose().to_owned();
    // The same ID, another secret of the same form.
    let (head, secret) = text.split_at(text.len() - 43);
    let wrong = format!(
        "{head}{}A",
        if secret.starts_with('A') { "B" } else { "A" }.repeat(42)
    );
    present(agent, &wrong, &from).await;
}

#[when(expr = "a program presents a token nobody created from {word}")]
async fn presents_unknown(agent: &mut Agent, from: String) {
    let unknown = format!("ladderback_aaaaaaaaaaaa_{}A", "A".repeat(42));
    present(agent, &unknown, &from).await;
}

#[then(expr = "the program is let in with the scopes {string}")]
fn let_in(agent: &mut Agent, scopes: String) {
    let checked = agent
        .api_tokens
        .checked
        .as_ref()
        .expect("a token was presented");
    let grant = checked.as_ref().expect("the token is accepted");
    assert_eq!(grant.scopes.iter().collect::<Vec<_>>(), scopes_of(&scopes));
    assert_eq!(grant.id, created(agent).token.id);
}

#[then("the token is refused as invalid")]
fn refused_invalid(agent: &mut Agent) {
    assert_eq!(
        agent
            .api_tokens
            .checked
            .as_ref()
            .and_then(|checked| checked.as_ref().err()),
        Some(&ApiTokenRefused::Invalid)
    );
}

#[then(expr = "the token is refused with a wait of {int} second(s)")]
fn refused_wait(agent: &mut Agent, seconds: u64) {
    assert_eq!(
        agent
            .api_tokens
            .checked
            .as_ref()
            .and_then(|checked| checked.as_ref().err()),
        Some(&ApiTokenRefused::Wait { seconds })
    );
}

#[when("the admin revokes the token")]
async fn revokes(agent: &mut Agent) {
    let id = created(agent).token.id.clone();
    let revoked = agent.sign_in.access().revoke_api_token(&id).await;
    assert_eq!(revoked, Ok(true));
}

#[then("no API token is listed")]
async fn none_listed(agent: &mut Agent) {
    assert!(listed(agent).await.is_empty());
}

#[then(expr = "the token was last used {int} seconds ago")]
async fn last_used_ago(agent: &mut Agent, seconds: i64) {
    let tokens = listed(agent).await;
    let last = tokens.first().and_then(|token| token.last_used_at);
    let now = agent.clock.now();
    assert_eq!(
        last.map(|last| now.duration_since(last).as_secs()),
        Some(seconds)
    );
}

#[then("the token was last used just now")]
async fn last_used_now(agent: &mut Agent) {
    last_used_ago(agent, 0).await;
}
