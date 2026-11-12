//! The `ServiceManager` contract (operations §12): what installing and
//! removing the service leaves behind, the same for the in-memory fake and
//! for each real manager.

use async_trait::async_trait;
use domain::service::ServicePath;

use crate::ports::{Removed, ServiceManager};

/// The manager under test, and what a person could see of it: whether the
/// service runs, and its file. The fake answers from memory; a real manager's
/// harness asks the manager and reads the file.
#[async_trait]
pub trait ServiceHarness: Send + Sync {
    fn manager(&self) -> &dyn ServiceManager;

    /// Two programs the service may run, which differ.
    fn programs(&self) -> [ServicePath; 2];

    /// Whether the manager runs the service.
    async fn runs(&self) -> bool;

    /// The text of the service file, or `None` when there is none.
    fn file_text(&self) -> Option<String>;
}

/// Installs, installs over it with the other program, removes, and removes
/// again, checking after each step what the port's contract says.
pub async fn run(harness: &dyn ServiceHarness) {
    let [first, second] = harness.programs();
    assert_ne!(first, second, "the harness gives two programs");
    let manager = harness.manager();

    let installed = manager.install(&first).await;
    assert!(installed.is_ok(), "the install failed with {installed:?}");
    let Ok(installed) = installed else { return };
    assert!(!installed.replaced, "nothing was installed before");
    assert!(harness.runs().await, "the installed service runs");
    let text = harness.file_text();
    assert!(text.is_some(), "the service file is written");
    let Some(text) = text else { return };
    assert!(
        text.contains(first.as_str()) && text.contains("serve"),
        "the service file runs the program's serve: {text}"
    );

    let again = manager.install(&second).await;
    assert!(again.is_ok(), "the second install failed with {again:?}");
    let Ok(again) = again else { return };
    assert!(again.replaced, "an install over the service replaces it");
    assert_eq!(again.file, installed.file, "the service has one file");
    assert!(harness.runs().await, "the service runs again");
    let text = harness.file_text();
    assert!(text.is_some(), "the service file is still there");
    let Some(text) = text else { return };
    assert!(
        text.contains(second.as_str()) && !text.contains(first.as_str()),
        "the service file runs the new program only: {text}"
    );

    let removed = manager.remove().await;
    assert!(removed.is_ok(), "the removal failed with {removed:?}");
    let Ok(removed) = removed else { return };
    assert_eq!(
        removed,
        Removed::Removed {
            file: installed.file.clone()
        }
    );
    assert!(!harness.runs().await, "the removed service does not run");
    assert_eq!(harness.file_text(), None, "the service file is gone");

    let again = manager.remove().await;
    assert!(again.is_ok(), "removing no service failed with {again:?}");
    let Ok(again) = again else { return };
    assert_eq!(
        again,
        Removed::NotInstalled {
            file: installed.file
        }
    );
    assert!(!harness.runs().await, "still nothing runs");
}
