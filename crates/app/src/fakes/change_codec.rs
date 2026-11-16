use std::sync::{Mutex, MutexGuard, PoisonError};

use domain::planning::ConfigChange;

use crate::ports::{ChangeCodec, CodecError};

/// Writes each list of changes as `changes-N`, its place in a list it keeps
/// in memory, and reads that back: a codec for use-case tests and
/// acceptance scenarios, which never need the file's format.
#[derive(Default)]
pub struct FakeChangeCodec {
    written: Mutex<Vec<Vec<ConfigChange>>>,
}

impl FakeChangeCodec {
    pub fn new() -> Self {
        Self::default()
    }

    fn written(&self) -> MutexGuard<'_, Vec<Vec<ConfigChange>>> {
        self.written.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ChangeCodec for FakeChangeCodec {
    fn encode(&self, changes: &[ConfigChange]) -> Result<String, CodecError> {
        let _ = (self.written(), changes);
        todo!()
    }

    fn decode(&self, text: &str) -> Result<Vec<ConfigChange>, CodecError> {
        let _ = text;
        todo!()
    }
}
