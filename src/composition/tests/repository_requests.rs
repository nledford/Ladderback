use super::support::*;
use super::*;

/// A start, a preview and a read of the groups name a repository by its
/// alias: one the configuration doesn't hold and one no alias could be
/// are both unknown, and the engine is never asked.
#[tokio::test]
async fn a_maintenance_request_for_a_repository_that_is_not_there_is_unknown() {
    use http_adapter::MaintenanceRequest as Request;
    let (runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    for repository in ["missing", "Not An Alias"] {
        for request in [Request::Forget, Request::Prune, Request::Check] {
            assert_eq!(
                runner.preview(repository.to_owned(), request.clone()).await,
                Err(MaintenanceRefusal::UnknownRepository),
                "a preview of {request:?} on {repository}"
            );
            assert_eq!(
                runner
                    .start_maintenance(repository.to_owned(), request.clone(), None)
                    .await,
                Err(MaintenanceRefusal::UnknownRepository),
                "a start of {request:?} on {repository}"
            );
        }
        assert_eq!(
            runner.groups(repository.to_owned()).await,
            Err(MaintenanceRefusal::UnknownRepository),
            "the groups of {repository}"
        );
        assert_eq!(
            runner.stats(repository.to_owned()).await,
            Err(MaintenanceRefusal::UnknownRepository),
            "the stats of {repository}"
        );
        assert_eq!(
            runner.read_stats(repository.to_owned()).await,
            Err(MaintenanceRefusal::UnknownRepository),
            "a read of the stats of {repository}"
        );
    }
}

/// A check says nothing of the repository but that it is configured, so
/// its preview needs no engine: this runner's parks every job.
#[tokio::test]
async fn a_check_preview_is_told_without_the_engine() {
    let (runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    assert_eq!(
        runner
            .preview("demo".to_owned(), http_adapter::MaintenanceRequest::Check)
            .await,
        Ok(api::MaintenancePreviewDto::Check { read_data: false })
    );
}

/// A selection is the text a preview named it by; anything else is
/// refused before the repository is read: this runner's engine parks.
#[tokio::test]
async fn a_selection_that_is_not_one_is_refused_before_anything_is_read() {
    let (runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    assert_eq!(
        runner
            .start_maintenance(
                "demo".to_owned(),
                http_adapter::MaintenanceRequest::Forget,
                Some("not-a-selection".to_owned()),
            )
            .await,
        Err(MaintenanceRefusal::InvalidBody)
    );
}

/// An adoption into a plan no plan could be called is unknown before the
/// repository is read.
#[tokio::test]
async fn an_adoption_into_a_plan_that_cannot_be_named_is_refused_before_anything_is_read() {
    let (runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    for into in ["Not A Plan", ""] {
        assert_eq!(
            runner
                .start_maintenance(
                    "demo".to_owned(),
                    http_adapter::MaintenanceRequest::Adopt {
                        into: into.to_owned(),
                        from: api::AdoptionFromDto::Unmanaged,
                    },
                    None,
                )
                .await,
            Err(MaintenanceRefusal::UnknownPlan {
                plan: into.to_owned()
            })
        );
    }
}

/// Every plan of the repository is listed, the ones with no snapshots
/// too, with the repository read live through the engine.
#[tokio::test]
async fn the_groups_of_a_repository_with_no_snapshots_still_list_every_plan() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(Empty);
    assert_eq!(
        runner.groups("demo".to_owned()).await,
        Ok(api::SnapshotGroupsDto {
            plans: vec![api::PlanGroupDto {
                plan: "home".to_owned(),
                count: 0,
                snapshots: Vec::new(),
            }],
            orphaned: Vec::new(),
            unmanaged: Vec::new(),
            warnings: Vec::new(),
        })
    );
}
