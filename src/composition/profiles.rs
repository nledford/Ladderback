//! The profile routes over the configuration: an import's preview and a
//! plan's export.

use async_trait::async_trait;
use http_adapter::{Profiles, ProfilesError};

/// Previews profile imports and exports plans.
pub(super) struct ProfileService;

#[async_trait]
impl Profiles for ProfileService {
    async fn preview(
        &self,
        _request: api::ProfilePreviewDto,
    ) -> Result<api::ProfileImportDto, ProfilesError> {
        todo!()
    }

    async fn export(&self, _plan: String) -> Result<api::ProfileExportDto, ProfilesError> {
        todo!()
    }
}
