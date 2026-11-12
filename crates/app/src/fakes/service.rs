//! A `ServiceManager` fake: the service file and whether the service runs,
//! in memory. A real manager's file format is its adapter's, so the fake's
//! file is the program and `serve`, as a line.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::service::ServicePath;

use crate::ports::{Installed, Removed, ServiceError, ServiceManager, ServiceProblem};

/// The service as the fake manager keeps it.
#[derive(Debug, Default)]
struct State {
    /// The service file's text, when there is one.
    file: Option<String>,
    runs: bool,
    /// What the manager says when it refuses the next request.
    refusal: Option<String>,
    /// How many times the manager was asked to start or stop the service.
    asked: usize,
}

/// Keeps the service in memory, at `file`.
pub struct FakeServiceManager {
    file: PathBuf,
    state: Mutex<State>,
}

impl FakeServiceManager {
    pub fn new(file: PathBuf) -> Self {
        Self {
            file,
            state: Mutex::new(State::default()),
        }
    }

    /// Makes the manager refuse what it is asked from now on, saying `words`.
    pub fn refuse(&self, words: &str) {
        self.state().refusal = Some(words.to_owned());
    }

    /// Whether the service runs.
    pub fn runs(&self) -> bool {
        self.state().runs
    }

    /// The service file's text, or `None` when there is none.
    pub fn file_text(&self) -> Option<String> {
        self.state().file.clone()
    }

    /// How many times the manager was asked to start or stop the service.
    pub fn asked(&self) -> usize {
        self.state().asked
    }

    /// Counts a request of the manager, and refuses it when told to.
    fn ask(&self, state: &mut State, command: &str) -> Result<(), ServiceError> {
        state.asked += 1;
        match &state.refusal {
            Some(said) => Err(ServiceError {
                file: self.file.clone(),
                problem: ServiceProblem::ManagerRefused {
                    command: command.to_owned(),
                    said: said.clone(),
                },
            }),
            None => Ok(()),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl ServiceManager for FakeServiceManager {
    async fn install(&self, program: &ServicePath) -> Result<Installed, ServiceError> {
        let mut state = self.state();
        let replaced = state.file.is_some();
        state.file = Some(format!("{} serve", program.as_str()));
        self.ask(&mut state, "start")?;
        state.runs = true;
        Ok(Installed {
            file: self.file.clone(),
            replaced,
        })
    }

    async fn remove(&self) -> Result<Removed, ServiceError> {
        let mut state = self.state();
        if state.file.is_none() {
            return Ok(Removed::NotInstalled {
                file: self.file.clone(),
            });
        }
        self.ask(&mut state, "stop")?;
        state.runs = false;
        state.file = None;
        Ok(Removed::Removed {
            file: self.file.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use domain::service::ServicePath;

    use super::FakeServiceManager;
    use crate::ports::{Removed, ServiceManager, ServiceProblem};

    fn fake() -> FakeServiceManager {
        FakeServiceManager::new(PathBuf::from("/home/demo/.config/ladderback.service"))
    }

    fn program() -> ServicePath {
        ServicePath::parse(Path::new("/home/demo/bin/ladderback")).unwrap()
    }

    #[tokio::test]
    async fn a_refusing_manager_starts_nothing_and_says_its_words() {
        let manager = fake();
        manager.refuse("Failed to connect to bus");
        let error = manager.install(&program()).await.unwrap_err();
        assert!(matches!(
            error.problem,
            ServiceProblem::ManagerRefused { ref said, .. } if said == "Failed to connect to bus"
        ));
        assert!(!manager.runs());
    }

    #[tokio::test]
    async fn removing_no_service_asks_the_manager_nothing() {
        let manager = fake();
        assert!(matches!(
            manager.remove().await,
            Ok(Removed::NotInstalled { .. })
        ));
        assert_eq!(manager.asked(), 0);
    }

    #[tokio::test]
    async fn an_install_and_a_removal_each_ask_the_manager_once() {
        let manager = fake();
        manager.install(&program()).await.unwrap();
        assert_eq!(manager.asked(), 1);
        manager.remove().await.unwrap();
        assert_eq!(manager.asked(), 2);
    }

    #[tokio::test]
    async fn a_refused_removal_keeps_the_service_and_its_file() {
        let manager = fake();
        manager.install(&program()).await.unwrap();
        manager.refuse("Access denied");
        assert!(manager.remove().await.is_err());
        assert!(manager.runs());
        assert!(manager.file_text().is_some());
    }
}
