//! The `ConfigDrafts` contract (decision 89, point 12): a proposal's apply
//! is shown the file it would write, and saves only over the file as it was
//! shown.

use std::sync::Arc;

use async_trait::async_trait;
use domain::PlanId;
use domain::access::TOKEN_BYTES;
use domain::planning::{AgentConfig, ConfigRevision, Plan, ProposalId, Source};
use domain::secrets::{SecretRef, SecretValue};

use crate::ports::{ConfigDraft, ConfigDrafts, ConfigStore, SavedOver};

/// A configuration in a place of its own, and a person who edits it by
/// hand.
#[async_trait]
pub trait DraftsHarness: Send + Sync {
    /// A store that holds nothing yet, as both ports, over one place.
    async fn empty(&self) -> (Arc<dyn ConfigStore>, Arc<dyn ConfigDrafts>);
    /// Changes how the place [`empty`](Self::empty) last gave writes the
    /// configuration, and nothing in it, as a comment edited by hand does.
    async fn edit_by_hand(&self);
}

pub async fn run(harness: &dyn DraftsHarness) {
    a_draft_writes_nothing_and_shows_what_a_save_writes(harness).await;
    a_save_over_the_file_shown_writes_it_and_names_the_proposal(harness).await;
    a_save_over_a_file_edited_since_writes_nothing(harness).await;
    the_diff_a_review_shows_holds_no_form_of_a_secret(harness).await;
}

fn proposal() -> ProposalId {
    ProposalId::from_bytes(&[3; TOKEN_BYTES])
}

/// The configuration [`minimal`](super::config_store::minimal) gives, and
/// the same with the plan `photos` added.
fn before_and_after() -> Option<(AgentConfig, AgentConfig)> {
    let before = super::config_store::minimal()?;
    let alias = before.repositories().keys().next()?.clone();
    let photos = Plan::new(
        PlanId::parse("photos").ok()?,
        alias,
        vec![Source::parse("/home/demo/photos").ok()?],
    );
    let after = before.add_plan(photos).ok()?;
    Some((before, after))
}

/// The suite's own configurations, which are valid.
fn configs(made: Option<(AgentConfig, AgentConfig)>) -> Option<(AgentConfig, AgentConfig)> {
    assert!(made.is_some(), "the suite's own configurations are valid");
    made
}

/// The draft of `config`, which is made.
async fn drafted(drafts: &dyn ConfigDrafts, config: &AgentConfig) -> Option<ConfigDraft> {
    let draft = drafts.draft(config).await;
    assert!(draft.is_ok(), "the draft is made: {draft:?}");
    draft.ok()
}

/// A store holding `before`, saved as the daemon saves it.
async fn holding(
    harness: &dyn DraftsHarness,
    before: &AgentConfig,
) -> (Arc<dyn ConfigStore>, Arc<dyn ConfigDrafts>) {
    let (store, drafts) = harness.empty().await;
    assert_eq!(store.save(before).await, Ok(()), "the first save");
    (store, drafts)
}

async fn a_draft_writes_nothing_and_shows_what_a_save_writes(harness: &dyn DraftsHarness) {
    let Some((before, after)) = configs(before_and_after()) else {
        return;
    };
    let (store, drafts) = holding(harness, &before).await;
    let Some(unchanged) = drafted(&*drafts, &before).await else {
        return;
    };
    assert_eq!(
        unchanged.after, unchanged.before,
        "the same configuration writes the same file"
    );
    assert_eq!(
        unchanged.revision,
        ConfigRevision::of(unchanged.before.as_bytes())
    );
    let Some(draft) = drafted(&*drafts, &after).await else {
        return;
    };
    assert_eq!(draft.before, unchanged.before);
    assert_ne!(
        draft.after, draft.before,
        "a plan added writes another file"
    );
    assert!(draft.after.contains("photos"), "{}", draft.after);
    assert_eq!(store.load().await, Ok(before), "a draft writes nothing");
}

async fn a_save_over_the_file_shown_writes_it_and_names_the_proposal(harness: &dyn DraftsHarness) {
    let Some((before, after)) = configs(before_and_after()) else {
        return;
    };
    let (store, drafts) = holding(harness, &before).await;
    let Some(draft) = drafted(&*drafts, &after).await else {
        return;
    };
    let saved = drafts.save_over(&draft.revision, &after, &proposal()).await;
    assert!(
        matches!(saved, Ok(SavedOver::Saved { kept: Some(_) })),
        "the save keeps the file as it was in the history: {saved:?}"
    );
    let Ok(SavedOver::Saved { kept: Some(kept) }) = saved else {
        return;
    };
    assert!(
        kept.contains(proposal().as_str()),
        "the history names the proposal: {kept}"
    );
    assert_eq!(store.load().await, Ok(after.clone()));
    let Some(now) = drafted(&*drafts, &after).await else {
        return;
    };
    assert_eq!(now.before, draft.after, "the file is what the draft showed");
}

async fn a_save_over_a_file_edited_since_writes_nothing(harness: &dyn DraftsHarness) {
    let Some((before, after)) = configs(before_and_after()) else {
        return;
    };
    let (store, drafts) = holding(harness, &before).await;
    let Some(draft) = drafted(&*drafts, &after).await else {
        return;
    };
    harness.edit_by_hand().await;
    assert_eq!(
        drafts.save_over(&draft.revision, &after, &proposal()).await,
        Ok(SavedOver::ChangedMeanwhile)
    );
    assert_eq!(store.load().await, Ok(before.clone()));
    let Some(edited) = drafted(&*drafts, &before).await else {
        return;
    };
    assert_ne!(edited.before, draft.before, "the hand edit is kept");
}

/// A password and a credential option that hold what a TOML string escapes
/// or splits: a quote, a backslash, a newline, a tab and a control
/// character. Each piece of them holds `canary`, so a diff that holds no
/// `canary` holds no form of them: raw, escaped or a line at a time.
const CANARY_PASSWORD: &str = "canary\"quote\\backslash\ncanary-second\tline";
const CANARY_KEY: &str = "canary-key\u{7}bell'single";

/// A repository holding [`CANARY_PASSWORD`] and [`CANARY_KEY`] as text and
/// no plan yet, and the same with the plan `photos` added: the diff's lines
/// around the plan are the repository's.
fn holding_canaries() -> Option<(AgentConfig, AgentConfig)> {
    let (before, _) = before_and_after()?;
    let (alias, repository) = before.repositories().iter().next()?;
    let mut repository = repository.clone();
    repository.settings.password = SecretRef::Inline(SecretValue::new(CANARY_PASSWORD));
    repository
        .settings
        .options
        .insert("access-key".to_owned(), CANARY_KEY.to_owned());
    let before = AgentConfig::new(
        before.agent().clone(),
        before.hooks().clone(),
        std::collections::BTreeMap::from([(alias.clone(), repository)]),
        Vec::new(),
    )
    .ok()?;
    let photos = Plan::new(
        PlanId::parse("photos").ok()?,
        alias.clone(),
        vec![Source::parse("/home/demo/photos").ok()?],
    );
    let after = before.add_plan(photos).ok()?;
    Some((before, after))
}

/// I4: a secret the file holds as text reaches neither text of a draft, in
/// any form, however the file escapes it, so the review's diff of them
/// holds none either.
async fn the_diff_a_review_shows_holds_no_form_of_a_secret(harness: &dyn DraftsHarness) {
    let Some((before, after)) = configs(holding_canaries()) else {
        return;
    };
    let (_store, drafts) = holding(harness, &before).await;
    let Some(draft) = drafted(&*drafts, &after).await else {
        return;
    };
    assert!(draft.after.contains("photos"), "{}", draft.after);
    assert!(!draft.before.contains("canary"), "{}", draft.before);
    assert!(!draft.after.contains("canary"), "{}", draft.after);
}
