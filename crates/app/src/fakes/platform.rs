use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use domain::execution::conditions::PowerSource;
use domain::planning::{CurrentNetworkName, Family, Link};

use crate::ports::{Network, NetworkLink, PlatformError, PowerSupply};

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

/// A network link that reports what it was set to: wired at first, on no
/// Wi-Fi network.
pub struct FakeNetworkLink {
    link: Scripted<Link>,
    name: Scripted<Option<CurrentNetworkName>>,
    ipv4_name: Scripted<Option<CurrentNetworkName>>,
    ipv6_name: Scripted<Option<CurrentNetworkName>>,
}

impl FakeNetworkLink {
    pub fn new() -> Self {
        Self {
            link: Scripted::new(Link::Wired),
            name: Scripted::new(None),
            ipv4_name: Scripted::new(None),
            ipv6_name: Scripted::new(None),
        }
    }

    /// What every later [`link`](NetworkLink::link) call answers.
    pub fn set(&self, answer: Result<Link, PlatformError>) {
        self.link.set(answer);
    }

    /// What every later [`network_name`](NetworkLink::network_name) call
    /// answers, of the link's interface and of each family's.
    pub fn set_name(&self, answer: Result<Option<CurrentNetworkName>, PlatformError>) {
        for family in [None, Some(Family::Ipv4), Some(Family::Ipv6)] {
            self.set_name_of(family, answer.clone());
        }
    }

    /// What every later [`network_name`](NetworkLink::network_name) call
    /// answers for `family`.
    pub fn set_name_of(
        &self,
        family: Option<Family>,
        answer: Result<Option<CurrentNetworkName>, PlatformError>,
    ) {
        self.names(family).set(answer);
    }

    /// How many times the link was read.
    pub fn asked(&self) -> usize {
        self.link.asked()
    }

    /// How many times a network's name was read, of any interface.
    pub fn names_asked(&self) -> usize {
        [&self.name, &self.ipv4_name, &self.ipv6_name]
            .iter()
            .fold(0, |sum, names| sum.saturating_add(names.asked()))
    }

    fn names(&self, family: Option<Family>) -> &Scripted<Option<CurrentNetworkName>> {
        match family {
            None => &self.name,
            Some(Family::Ipv4) => &self.ipv4_name,
            Some(Family::Ipv6) => &self.ipv6_name,
        }
    }
}

impl Default for FakeNetworkLink {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkLink for FakeNetworkLink {
    async fn link(&self) -> Result<Link, PlatformError> {
        self.link.ask()
    }

    async fn network_name(
        &self,
        family: Option<Family>,
    ) -> Result<Option<CurrentNetworkName>, PlatformError> {
        self.names(family).ask()
    }
}
