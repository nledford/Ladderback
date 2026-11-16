use domain::planning::ConfigChange;

/// Writes configuration changes as text and reads them back (decision 89),
/// so a proposal's changes can be kept by a store that knows no
/// configuration format. The configuration adapter writes each plan as the
/// configuration file writes it, and reads it back as the file is read when
/// it loads: text a store kept is read with the same care as a file.
///
/// Contract (checked by `contract::change_codec`):
/// - [`decode`](Self::decode) of what [`encode`](Self::encode) made gives
///   the same changes, in the same order;
/// - text it didn't make is a [`CodecError`], never a guess.
pub trait ChangeCodec: Send + Sync {
    fn encode(&self, changes: &[ConfigChange]) -> Result<String, CodecError>;

    fn decode(&self, text: &str) -> Result<Vec<ConfigChange>, CodecError>;
}

/// Changes that couldn't be written, or text that isn't changes; the text
/// is the codec's own words, which name no value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecError(pub String);
