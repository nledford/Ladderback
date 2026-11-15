//! The profile routes' handle (technical design §4.6, §6.2): a preview of a
//! profile's import from its text, checked as `import-profile` checks it and
//! answered as its `--json` document, and a plan exported as
//! `export-profile` exports it. Neither saves anything.

use std::path::PathBuf;
use std::sync::Arc;

use app::interop::{ExportError, ExportProfile, ImportError, ImportProfile};
use app::ports::{ConfigError, ConfigStore, NormalizeSources};
use async_trait::async_trait;
use domain::{AgentId, PlanId, RepositoryAlias};
use http_adapter::{Profiles, ProfilesError};
use interop::{FileProfileReader, KeyringCommand, RusticProfileWriter};

use super::{Notice, Refusal, export_refusal, import_refusal, profile_notice};

/// The name a refusal gives a profile previewed from its text, where it
/// would name a profile's file.
const PREVIEWED: &str = "content";

/// The daemon's [`Profiles`]: the interop use cases over its configuration.
pub(super) struct ProfileService {
    import: ImportProfile,
    export: ExportProfile,
    /// The configuration's own file, which a refusal about the configuration
    /// an import would make names, as `import-profile` names it.
    config_file: PathBuf,
}

impl ProfileService {
    /// The handle over `config`, kept in `config_file`, for the agent
    /// `agent`. A preview reads no folder for the profiles a profile uses
    /// and none of rustic's variables: what it is given is all it reads.
    /// An export names the keyring command found on the daemon's `PATH`.
    pub(super) fn new(
        config: Arc<dyn ConfigStore>,
        config_file: PathBuf,
        agent: AgentId,
        sources: Arc<dyn NormalizeSources>,
    ) -> Self {
        let reader = FileProfileReader::new(Vec::new(), []);
        let path = std::env::var_os("PATH");
        let writer = RusticProfileWriter::new(KeyringCommand::of_this_platform(), path.as_deref());
        Self {
            import: ImportProfile::new(Arc::new(reader), Arc::clone(&config), sources),
            export: ExportProfile::new(Arc::new(writer), config, agent),
            config_file,
        }
    }
}

#[async_trait]
impl Profiles for ProfileService {
    async fn preview(
        &self,
        request: api::ProfilePreviewDto,
    ) -> Result<api::ProfileImportDto, ProfilesError> {
        let Ok(alias) = RepositoryAlias::parse(&request.repository) else {
            return Err(refused(super::repository_alias_invalid()));
        };
        match self
            .import
            .preview(PREVIEWED, &request.content, alias)
            .await
        {
            Ok(imported) => Ok(crate::profiles::import_document(imported)),
            // The configuration the import would make breaks one of its
            // rules: the profile can't be carried over. Any other mistake is
            // in the file as it is, which nothing can be imported into.
            Err(ImportError::Config(error)) if !made_by_the_import(&error) => Err(unavailable(
                import_refusal(ImportError::Config(error), &self.config_file),
            )),
            Err(error) => Err(refused(import_refusal(error, &self.config_file))),
        }
    }

    async fn export(&self, plan: String) -> Result<api::ProfileExportDto, ProfilesError> {
        // A name that is not a plan ID is not a plan of the configuration.
        let Ok(id) = PlanId::parse(&plan) else {
            return Err(ProfilesError::PlanUnknown { plan });
        };
        match self.export.export(&id).await {
            Ok(exported) => Ok(api::ProfileExportDto {
                text: exported.text,
                notices: exported
                    .notices
                    .into_iter()
                    .map(|notice| {
                        let Notice { code, args } = profile_notice(notice);
                        api::WarningDto::new(code, args)
                    })
                    .collect(),
            }),
            Err(ExportError::PlanUnknown(_)) => Err(ProfilesError::PlanUnknown { plan }),
            Err(error @ ExportError::Config(_)) => {
                Err(unavailable(export_refusal(error, &self.config_file)))
            }
            Err(error) => Err(refused(export_refusal(error, &self.config_file))),
        }
    }
}

/// Whether `error` is about the configuration an import would make rather
/// than the file as it is: such a configuration was never read, so it has
/// no file of its own.
fn made_by_the_import(error: &ConfigError) -> bool {
    matches!(error, ConfigError::Invalid { file, .. } if file.is_empty())
}

/// A profile or plan that can't be carried over.
fn refused(Refusal { code, args, detail }: Refusal) -> ProfilesError {
    ProfilesError::Refused { code, args, detail }
}

/// A configuration that can't be read or used just now.
fn unavailable(Refusal { code, args, detail }: Refusal) -> ProfilesError {
    ProfilesError::Unavailable { code, args, detail }
}
