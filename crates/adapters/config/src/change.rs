//! A proposal's changes as the store keeps them (decision 89): a JSON array
//! of the changes as the wire writes them, each plan as its table in the
//! file's keys. They are read back as the file is read when it loads, so
//! text a store kept is read with the same care as a file.

use app::ports::{ChangeCodec, CodecError};
use domain::planning::ConfigChange;

/// Writes a proposal's changes as JSON and reads them back.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonChangeCodec;

impl ChangeCodec for JsonChangeCodec {
    fn encode(&self, changes: &[ConfigChange]) -> Result<String, CodecError> {
        let _ = changes;
        todo!()
    }

    fn decode(&self, text: &str) -> Result<Vec<ConfigChange>, CodecError> {
        let _ = text;
        todo!()
    }
}
