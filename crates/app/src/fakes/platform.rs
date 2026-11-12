use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use domain::execution::conditions::PowerSource;

use crate::ports::{Network, PlatformError, PowerSupply};

/// One scripted answer, and how often it was asked for.
struct Scripted<T> {
    answer: Mutex<Result<T, PlatformError>>,
    asked: AtomicUsize,
}

impl<T: Clone> Scripted<T> {
    fn new(answer: T) -> Self {
        Self {
            answer: Mutex::new(Ok(answer)),
            asked: AtomicUsize::new(0),
        }
    }

    fn set(&self, answer: Result<T, PlatformError>) {
        *self.answer.lock().unwrap_or_else(|e| e.into_inner()) = answer;
    }

    fn ask(&self) -> Result<T, PlatformError> {
        self.asked.fetch_add(1, Ordering::Relaxed);
        self.answer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::Relaxed)
    }
}

/// A power supply that reports what it was set to: AC power at first.
pub struct FakePower(Scripted<PowerSource>);

impl FakePower {
    pub fn new() -> Self {
        Self(Scripted::new(PowerSource::Ac))
    }

    /// What every later [`source`](PowerSupply::source) call answers.
    pub fn set(&self, answer: Result<PowerSource, PlatformError>) {
        self.0.set(answer);
    }

    /// How many times the source was asked for.
    pub fn asked(&self) -> usize {
        self.0.asked()
    }
}

impl Default for FakePower {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PowerSupply for FakePower {
    async fn source(&self) -> Result<PowerSource, PlatformError> {
        self.0.ask()
    }
}

/// A network that reports what it was set to: online at first.
pub struct FakeNetwork(Scripted<bool>);

impl FakeNetwork {
    pub fn new() -> Self {
        Self(Scripted::new(true))
    }

    /// What every later [`is_online`](Network::is_online) call answers.
    pub fn set(&self, answer: Result<bool, PlatformError>) {
        self.0.set(answer);
    }

    /// How many times the network was asked about.
    pub fn asked(&self) -> usize {
        self.0.asked()
    }
}

impl Default for FakeNetwork {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Network for FakeNetwork {
    async fn is_online(&self) -> Result<bool, PlatformError> {
        self.0.ask()
    }
}
