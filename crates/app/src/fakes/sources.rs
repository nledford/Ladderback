//! A `NormalizeSources` fake for the acceptance runner: sources as written,
//! which is what the scripted listing reports.

use async_trait::async_trait;
use domain::planning::Source;

use crate::ports::NormalizeSources;

#[derive(Default)]
pub struct FakeSources;

#[async_trait]
impl NormalizeSources for FakeSources {
    async fn normalized(
        &self,
        sources: &[Source],
    ) -> Result<Vec<Source>, crate::ports::SourcesError> {
        Ok(sources.to_vec())
    }
}
