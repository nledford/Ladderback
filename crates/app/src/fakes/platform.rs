use async_trait::async_trait;
use domain::execution::conditions::PowerSource;

use crate::ports::{Network, PlatformError, PowerSupply};

/// A power supply that reports what it was set to: AC power at first.
pub struct FakePower {}

impl FakePower {
    pub fn new() -> Self {
        todo!()
    }

    /// What every later [`source`](PowerSupply::source) call answers.
    pub fn set(&self, answer: Result<PowerSource, PlatformError>) {
        let _ = answer;
        todo!()
    }

    /// How many times the source was asked for.
    pub fn asked(&self) -> usize {
        todo!()
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
        todo!()
    }
}

/// A network that reports what it was set to: online at first.
pub struct FakeNetwork {}

impl FakeNetwork {
    pub fn new() -> Self {
        todo!()
    }

    /// What every later [`is_online`](Network::is_online) call answers.
    pub fn set(&self, answer: Result<bool, PlatformError>) {
        let _ = answer;
        todo!()
    }

    /// How many times the network was asked about.
    pub fn asked(&self) -> usize {
        todo!()
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
        todo!()
    }
}
