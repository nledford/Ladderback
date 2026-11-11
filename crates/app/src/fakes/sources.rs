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

#[cfg(test)]
mod tests {
    use domain::planning::Source;

    use super::FakeSources;
    use crate::ports::NormalizeSources;

    #[tokio::test]
    async fn sources_come_back_as_written() {
        // Not normalized: a dot, a repeat and the order all stay as written.
        let written: Vec<Source> = ["/srv/demo/photos", "/srv/demo/./music", "/srv/demo/photos"]
            .map(|text| Source::parse(text).expect("a demo source"))
            .into();
        assert_eq!(FakeSources.normalized(&written).await, Ok(written));
    }
}
