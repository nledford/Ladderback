//! The `ChangeCodec` contract (decision 89): a proposal's changes, written
//! as text and read back the same.

use domain::planning::{ConfigChange, CronExpression, Overlap, Plan, Schedule, Source};
use domain::{PlanId, RepositoryAlias};

use crate::ports::ChangeCodec;

pub fn run(codec: &dyn ChangeCodec) {
    changes_read_back_as_they_were_written(codec);
    no_changes_read_back_as_none(codec);
    text_it_did_not_write_is_refused(codec);
}

/// A plan with settings beyond the ones every plan has.
fn plan(name: &str) -> Option<Plan> {
    let id = PlanId::parse(name);
    let repository = RepositoryAlias::parse("demo");
    let source = Source::parse(&format!("/home/demo/{name}"));
    let cron = CronExpression::parse("0 3 * * *");
    assert!(
        id.is_ok() && repository.is_ok() && source.is_ok() && cron.is_ok(),
        "the suite's own plan is valid"
    );
    let mut plan = Plan::new(id.ok()?, repository.ok()?, vec![source.ok()?]);
    plan.label = Some("Demo files".to_owned());
    plan.schedule = Some(Schedule::new(cron.ok()?));
    plan.overlap = Overlap::Queue;
    Some(plan)
}

fn changes_read_back_as_they_were_written(codec: &dyn ChangeCodec) {
    let (Some(added), Some(changed), Ok(removed)) =
        (plan("home"), plan("docs"), PlanId::parse("photos"))
    else {
        return;
    };
    let changes = vec![
        ConfigChange::AddPlan(added),
        ConfigChange::ChangePlan(changed),
        ConfigChange::RemovePlan(removed),
    ];
    let text = codec.encode(&changes);
    assert!(text.is_ok(), "the codec writes the changes: {text:?}");
    let read = codec.decode(&text.unwrap_or_default());
    assert_eq!(read, Ok(changes));
}

fn no_changes_read_back_as_none(codec: &dyn ChangeCodec) {
    let text = codec.encode(&[]);
    assert!(text.is_ok(), "the codec writes no changes: {text:?}");
    assert_eq!(codec.decode(&text.unwrap_or_default()), Ok(Vec::new()));
}

fn text_it_did_not_write_is_refused(codec: &dyn ChangeCodec) {
    for text in ["", "not changes", "[{\"kind\":\"rename-plan\"}]"] {
        let read = codec.decode(text);
        assert!(read.is_err(), "{text:?} is refused, not {read:?}");
    }
}
