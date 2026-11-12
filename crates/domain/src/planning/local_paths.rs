//! The paths on this machine that a repository's settings name. rustic, the
//! programs it starts, the hooks and the password command run in the
//! daemon's working directory (`/` under launchd or a system unit, the home
//! folder under a user unit), never the folder `config.toml` is in, so a
//! relative path there would name a different file, or none, depending on
//! how the daemon was started.
//!
//! A field is judged by what is known of it before a job resolves its
//! `${…}` references: the text up to the first one. What a reference holds
//! is checked by nothing here.

use crate::secrets::{CommandLine, Part, SecretRef, Template};

use super::RepositorySettings;

/// A setting of a repository, by the key it is written at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepositoryField {
    Repository,
    RepoHot,
    /// The program of the password's command (`password-command`).
    PasswordCommand,
    /// An entry of `options`, by its name.
    Option(String),
    OptionHot(String),
    OptionCold(String),
}

impl RepositorySettings {
    /// The first setting that names a file or folder on this machine by a
    /// relative path, in the order `repository`, `repo-hot`,
    /// `password-command`, `options`, `options-hot`, `options-cold`, each
    /// set by name.
    pub fn relative_path(&self) -> Option<RepositoryField> {
        let cold = location(&self.repository);
        if cold.as_ref().is_some_and(|cold| cold.relative) {
            return Some(RepositoryField::Repository);
        }
        let hot = self.repo_hot.as_deref().and_then(location);
        if hot.as_ref().is_some_and(|hot| hot.relative) {
            return Some(RepositoryField::RepoHot);
        }
        if let SecretRef::Command(command) = &self.password
            && is_relative_program(command.program())
        {
            return Some(RepositoryField::PasswordCommand);
        }
        let cold = cold.map(|cold| cold.backend);
        let hot = hot.map(|hot| hot.backend);
        let reads = |backend: &Option<Backend>, name: &str, value: &str| {
            backend
                .as_ref()
                .is_some_and(|backend| backend.reads_relative(name, value))
        };
        // A shared option counts for each half that doesn't set its own.
        for (name, value) in &self.options {
            if (!self.options_cold.contains_key(name) && reads(&cold, name, value))
                || (!self.options_hot.contains_key(name) && reads(&hot, name, value))
            {
                return Some(RepositoryField::Option(name.clone()));
            }
        }
        if let Some((name, _)) = self
            .options_hot
            .iter()
            .find(|(name, value)| reads(&hot, name, value))
        {
            return Some(RepositoryField::OptionHot(name.clone()));
        }
        self.options_cold
            .iter()
            .find(|(name, value)| reads(&cold, name, value))
            .map(|(name, _)| RepositoryField::OptionCold(name.clone()))
    }
}

/// Whether `location`, a repository location as a job resolved it (every
/// reference replaced by what it held), names a folder on this machine by a
/// relative path, read as [`RepositorySettings::relative_path`] reads a
/// location all of which is known.
pub fn is_relative_location(_location: &str) -> bool {
    todo!()
}

/// Whether a program is named by a relative path: with a `/` in it, and not
/// at its start. A name without one is looked up on `PATH`.
pub(super) fn is_relative_program(program: &str) -> bool {
    program.contains('/') && !program.starts_with('/')
}

/// What a field is known to hold before a job resolves it: the text before
/// its first reference, and whether that is all of it. Nothing is known of a
/// field that starts with a reference, or that is no template, which a job
/// refuses.
struct Known {
    head: String,
    whole: bool,
}

impl Known {
    fn of(value: &str) -> Option<Self> {
        let template = Template::parse(value).ok()?;
        let mut parts = template.parts().iter();
        match parts.next() {
            None => Some(Self {
                head: String::new(),
                whole: true,
            }),
            Some(Part::Text(head)) => Some(Self {
                head: head.clone(),
                whole: parts.next().is_none(),
            }),
            Some(Part::Reference(_)) => None,
        }
    }
}

/// Whether a path is relative, by `head`, what it starts with. An empty one
/// is relative only when it is all there is.
fn starts_relative(head: &str, whole: bool) -> bool {
    if head.is_empty() {
        whole
    } else {
        !head.starts_with('/')
    }
}

/// Whether `value`, a path, is known to be relative.
fn is_relative_file(value: &str) -> bool {
    Known::of(value).is_some_and(|known| starts_relative(&known.head, known.whole))
}

/// The backend rustic reaches a repository through, by its location's scheme.
enum Backend {
    Local,
    Rclone,
    Rest,
    /// The service, in lower case, unless a reference holds it.
    OpenDal(Option<String>),
}

struct Location {
    backend: Backend,
    /// Whether it is a local path, and relative.
    relative: bool,
}

/// A location as rustic reads it: the scheme before the first `:`, else a
/// local path. Nothing when a reference may still decide the backend, or
/// when rustic refuses the scheme.
fn location(value: &str) -> Option<Location> {
    let Known { head, whole } = Known::of(value)?;
    let Some((scheme, rest)) = head.split_once(':') else {
        // A `/` before any `:` leaves no scheme a reference could finish.
        return (whole || head.contains('/')).then(|| Location {
            backend: Backend::Local,
            relative: starts_relative(&head, whole),
        });
    };
    let (backend, relative) = match scheme {
        "local" => (Backend::Local, starts_relative(rest, whole)),
        "rclone" => (Backend::Rclone, rclone_folder_relative(rest, whole)),
        "rest" => (Backend::Rest, false),
        // rustic gives OpenDAL the text up to the next `:` as the service.
        "opendal" => {
            let service = match rest.split_once(':') {
                Some((service, _)) => Some(service),
                None => whole.then_some(rest),
            };
            (
                Backend::OpenDal(service.map(str::to_ascii_lowercase)),
                false,
            )
        }
        _ => return None,
    };
    Some(Location { backend, relative })
}

/// Whether rclone reads `path`, what a location has after `rclone:`, as a
/// folder on this machine named by a relative path: one with no `remote:`
/// before its first `/` (a remote's name has no `/`), and not starting with
/// one. Without either, a reference could still make it a remote.
fn rclone_folder_relative(path: &str, whole: bool) -> bool {
    match path.find([':', '/']) {
        Some(at) => at > 0 && path[at..].starts_with('/'),
        None => whole,
    }
}

impl Backend {
    /// Whether the option `name`, which this backend reads, names a file or
    /// runs a program by a relative path when it is `value`. rustic matches
    /// its own option names exactly; OpenDAL's, in any case.
    fn reads_relative(&self, name: &str, value: &str) -> bool {
        match (self, name) {
            (
                Self::Rclone | Self::Rest | Self::OpenDal(_),
                "http-cacert" | "http-tls-client-cert",
            ) => is_relative_file(value),
            (Self::Rclone, "rclone-command") => runs_relative(value, true),
            (Self::Local, "post-create-command" | "post-delete-command") => {
                runs_relative(value, false)
            }
            (Self::OpenDal(Some(service)), name) => {
                let name = name.to_ascii_lowercase();
                is_opendal_file(service, &name)
                    && is_relative_file(value)
                    // ssh expands a `~` in the key it is given (`-i`) itself.
                    && !(service == "sftp" && value.starts_with('~'))
            }
            _ => false,
        }
    }
}

/// The options of an OpenDAL service that name a file on this machine, by
/// every name OpenDAL takes for them.
fn is_opendal_file(service: &str, name: &str) -> bool {
    matches!(
        (service, name),
        ("fs" | "file", "root" | "atomic_write_dir")
            | ("sftp", "key")
            | ("gcs", "credential_path" | "google_application_credentials")
            | ("oss", "oidc_token_file")
    )
}

/// Whether the command line `value` starts its program by a relative path,
/// or, for rclone, names its configuration file by one (`--config`). A
/// command with a reference in it is split only once the job resolves it,
/// and one that can't be split is rustic's to refuse.
fn runs_relative(value: &str, rclone: bool) -> bool {
    let Ok(template) = Template::parse(value) else {
        return false;
    };
    // An empty command has no program to split.
    let [Part::Text(text)] = template.parts() else {
        return false;
    };
    let Ok(command) = CommandLine::parse(text) else {
        return false;
    };
    if is_relative_program(command.program()) {
        return true;
    }
    let mut args = command.args().iter();
    while rclone && let Some(arg) = args.next() {
        let config = if arg == "--config" {
            args.next().map(String::as_str)
        } else {
            arg.strip_prefix("--config=")
        };
        if config.is_some_and(|path| !path.is_empty() && !path.starts_with('/')) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::secrets::{CommandLine, SecretRef};

    fn at(repository: &str) -> RepositorySettings {
        RepositorySettings {
            repository: repository.to_owned(),
            repo_hot: None,
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        }
    }

    fn with_option(repository: &str, name: &str, value: &str) -> RepositorySettings {
        let mut settings = at(repository);
        settings.options.insert(name.to_owned(), value.to_owned());
        settings
    }

    fn option(name: &str) -> Option<RepositoryField> {
        Some(RepositoryField::Option(name.to_owned()))
    }

    /// Paths rustic reads from the working directory, as a local repository
    /// or a file an option names.
    const RELATIVE: [&str; 7] = [
        "backups/repo",
        "repo",
        "./repo",
        "../repo",
        "~/repo",
        "",
        "$${HOME}/repo",
    ];

    #[test]
    fn a_local_repository_written_as_a_relative_path_is_found() {
        for location in RELATIVE
            .into_iter()
            .map(str::to_owned)
            .chain(RELATIVE.iter().map(|path| format!("local:{path}")))
            .chain(["backups/${NAME}".to_owned()])
        {
            assert_eq!(
                at(&location).relative_path(),
                Some(RepositoryField::Repository),
                "{location:?}"
            );
            let mut hot = at("/srv/demo-repo");
            hot.repo_hot = Some(location.clone());
            assert_eq!(
                hot.relative_path(),
                Some(RepositoryField::RepoHot),
                "{location:?}"
            );
        }
    }

    #[test]
    fn an_rclone_location_rclone_reads_as_a_relative_folder_is_found() {
        // rclone serves a path with no `remote:` before its first `/` as a
        // folder on this machine.
        for location in [
            "rclone:backups/repo",
            "rclone:./repo",
            "rclone:../repo",
            "rclone:~/repo",
            "rclone:repo",
            "rclone:backups/${NAME}",
            "rclone:backups/nas:repo",
        ] {
            assert_eq!(
                at(location).relative_path(),
                Some(RepositoryField::Repository),
                "{location:?}"
            );
            let mut hot = at("/srv/demo-repo");
            hot.repo_hot = Some(location.to_owned());
            assert_eq!(
                hot.relative_path(),
                Some(RepositoryField::RepoHot),
                "{location:?}"
            );
        }
    }

    #[test]
    fn a_location_that_is_not_a_relative_path_on_this_machine_is_not_found() {
        for location in [
            "/srv/demo-repo",
            "local:/srv/demo-repo",
            "rclone:nas:backups/repo",
            "rest:https://nas.example/repo",
            "opendal:s3",
            "opendal:fs",
            // What rustic reads as a backend it doesn't know, and refuses.
            "nas:backups/repo",
            "LOCAL:backups/repo",
            // Resolved when a job starts; what they hold isn't known here.
            "${DEMO_REPOSITORY}",
            "${HOME}/repo",
            "local:${DEMO_REPOSITORY}",
            "/srv/${NAME}",
            "rclone:/srv/demo-repo",
            "rclone:nas:repo",
            "rclone:nas:${NAME}",
            "rclone:${DEMO_REMOTE}",
            "rclone:nas${NAME}",
        ] {
            assert_eq!(at(location).relative_path(), None, "{location:?}");
            let mut hot = at("/srv/demo-repo");
            hot.repo_hot = Some(location.to_owned());
            assert_eq!(hot.relative_path(), None, "{location:?}");
        }
    }

    #[test]
    fn a_file_an_option_names_by_a_relative_path_is_found_where_the_backend_reads_it() {
        for (location, name) in [
            ("rest:https://nas.example/repo", "http-cacert"),
            ("rest:https://nas.example/repo", "http-tls-client-cert"),
            ("opendal:s3", "http-cacert"),
            ("rclone:nas:backups", "http-tls-client-cert"),
            ("opendal:fs", "root"),
            ("opendal:FS", "root"),
            ("opendal:fs", "ROOT"),
            ("opendal:fs", "atomic_write_dir"),
            // OpenDAL's other name for its file system service, and what
            // follows a second `:`, which rustic doesn't read as the service.
            ("opendal:file", "root"),
            ("opendal:FILE", "atomic_write_dir"),
            ("opendal:fs:demo", "root"),
            ("opendal:fs:${NAME}", "root"),
            ("opendal:gcs", "credential_path"),
            ("opendal:gcs", "google_application_credentials"),
            ("opendal:oss", "oidc_token_file"),
        ] {
            for path in RELATIVE.into_iter().chain(["certs/${NAME}.pem"]) {
                assert_eq!(
                    with_option(location, name, path).relative_path(),
                    option(name),
                    "{location} {name} = {path:?}"
                );
            }
            for path in ["/etc/demo/file", "${DEMO_PATH}", "${HOME}/file"] {
                assert_eq!(
                    with_option(location, name, path).relative_path(),
                    None,
                    "{location} {name} = {path:?}"
                );
            }
        }
    }

    #[test]
    fn an_sftp_key_may_start_at_a_home_folder_as_ssh_expands_it() {
        for key in RELATIVE.into_iter().filter(|key| !key.starts_with('~')) {
            assert_eq!(
                with_option("opendal:sftp", "key", key).relative_path(),
                option("key"),
                "{key:?}"
            );
        }
        for key in [
            "~/.ssh/id_ed25519",
            "~demo/.ssh/id_ed25519",
            "/etc/demo/key",
        ] {
            assert_eq!(
                with_option("opendal:sftp", "key", key).relative_path(),
                None,
                "{key:?}"
            );
        }
    }

    #[test]
    fn an_option_a_backend_doesnt_read_as_a_file_is_not_a_path() {
        for (location, name) in [
            // A root inside a bucket or on a server, not on this machine.
            ("opendal:s3", "root"),
            ("opendal:sftp", "root"),
            ("opendal:webhdfs", "atomic_write_dir"),
            // rustic's local backend ignores the HTTP options.
            ("/srv/demo-repo", "http-cacert"),
            // rustic matches its own option names exactly.
            ("rest:https://nas.example/repo", "HTTP-CACERT"),
            ("rest:https://nas.example/repo", "http_cacert"),
            ("opendal:gcs", "key"),
            ("opendal:fs", "endpoint"),
            // The service isn't known until a job starts.
            ("opendal:${DEMO_SERVICE}", "root"),
            ("opendal:fs${NAME}", "root"),
            ("${DEMO_REPOSITORY}", "root"),
        ] {
            assert_eq!(
                with_option(location, name, "backups/demo").relative_path(),
                None,
                "{location} {name}"
            );
        }
    }

    #[test]
    fn a_command_an_option_runs_by_a_relative_path_is_found() {
        for (location, name, command) in [
            (
                "rclone:nas:backups",
                "rclone-command",
                "./rclone serve restic",
            ),
            (
                "rclone:nas:backups",
                "rclone-command",
                "bin/rclone serve restic",
            ),
            (
                "rclone:nas:backups",
                "rclone-command",
                "rclone --config rclone.conf serve restic",
            ),
            (
                "rclone:nas:backups",
                "rclone-command",
                "rclone serve restic --config=conf/rclone.conf",
            ),
            ("/srv/demo-repo", "post-create-command", "./notify created"),
            (
                "local:/srv/demo-repo",
                "post-delete-command",
                "~/bin/notify",
            ),
            (
                "local:${DEMO_REPOSITORY}",
                "post-create-command",
                "./notify created",
            ),
        ] {
            assert_eq!(
                with_option(location, name, command).relative_path(),
                option(name),
                "{location} {name} = {command:?}"
            );
        }
        for (location, name, command) in [
            (
                "rclone:nas:backups",
                "rclone-command",
                "rclone serve restic",
            ),
            (
                "rclone:nas:backups",
                "rclone-command",
                "/usr/local/bin/rclone --config /etc/rclone.conf serve restic",
            ),
            (
                "rclone:nas:backups",
                "rclone-command",
                "rclone --config= serve restic",
            ),
            // An argument other than rclone's config is the program's own.
            (
                "/srv/demo-repo",
                "post-create-command",
                "notify logs/created",
            ),
            // Run only by the backend that reads the option.
            (
                "rest:https://nas.example/repo",
                "post-create-command",
                "./notify",
            ),
            ("/srv/demo-repo", "rclone-command", "./rclone serve restic"),
            // A command that can't be split is rustic's to refuse.
            (
                "/srv/demo-repo",
                "post-create-command",
                "./notify 'never closed",
            ),
        ] {
            assert_eq!(
                with_option(location, name, command).relative_path(),
                None,
                "{location} {name} = {command:?}"
            );
        }
    }

    #[test]
    fn a_password_command_that_starts_a_program_by_a_relative_path_is_found() {
        let command = |text: &str| {
            let mut settings = at("/srv/demo-repo");
            settings.password = SecretRef::Command(CommandLine::parse(text).unwrap());
            settings
        };
        for text in [
            "./get-pass",
            "bin/get-pass --vault demo",
            "../get-pass",
            "~/bin/get-pass",
        ] {
            assert_eq!(
                command(text).relative_path(),
                Some(RepositoryField::PasswordCommand),
                "{text:?}"
            );
        }
        // A name is looked up on `PATH`; an absolute path is one file; an
        // argument is the program's own.
        for text in [
            "get-pass",
            "~get-pass",
            "/usr/local/bin/get-pass",
            "get-pass vault/demo",
        ] {
            assert_eq!(command(text).relative_path(), None, "{text:?}");
        }
    }

    #[test]
    fn an_option_counts_for_each_half_that_reads_it_and_doesnt_set_its_own() {
        let root = |value: &str| BTreeMap::from([("root".to_owned(), value.to_owned())]);
        // The cold half sets its own root, and there is no hot half.
        let mut settings = with_option("opendal:fs", "root", "backups");
        settings.options_cold = root("/srv/cold");
        assert_eq!(settings.relative_path(), None);
        // A hot half on the same service reads the shared one.
        settings.repo_hot = Some("opendal:fs".to_owned());
        assert_eq!(settings.relative_path(), option("root"));
        settings.options_hot = root("/srv/hot");
        assert_eq!(settings.relative_path(), None);
        // A hot option counts only where there is a hot half.
        let mut settings = at("opendal:fs");
        settings.options_hot = root("hot");
        assert_eq!(settings.relative_path(), None);
        settings.repo_hot = Some("opendal:fs".to_owned());
        assert_eq!(
            settings.relative_path(),
            Some(RepositoryField::OptionHot("root".to_owned()))
        );
        let mut settings = at("opendal:fs");
        settings.options_cold = root("cold");
        assert_eq!(
            settings.relative_path(),
            Some(RepositoryField::OptionCold("root".to_owned()))
        );
        // Only the hot half is on the local disk.
        let mut settings = with_option("opendal:s3", "root", "backups");
        assert_eq!(settings.relative_path(), None);
        settings.repo_hot = Some("opendal:fs".to_owned());
        assert_eq!(settings.relative_path(), option("root"));
    }

    #[test]
    fn the_first_relative_path_is_found_in_the_order_the_settings_are_written() {
        let mut settings = with_option("backups/cold", "root", "cold");
        settings.repo_hot = Some("backups/hot".to_owned());
        settings.password = SecretRef::Command(CommandLine::parse("./get-pass").unwrap());
        assert_eq!(settings.relative_path(), Some(RepositoryField::Repository));
        settings.repository = "opendal:fs".to_owned();
        assert_eq!(settings.relative_path(), Some(RepositoryField::RepoHot));
        settings.repo_hot = Some("opendal:fs".to_owned());
        assert_eq!(
            settings.relative_path(),
            Some(RepositoryField::PasswordCommand)
        );
        settings.password = SecretRef::Env("DEMO_PASSWORD".to_owned());
        settings.options_hot = BTreeMap::from([("root".to_owned(), "hot".to_owned())]);
        assert_eq!(settings.relative_path(), option("root"));
        settings.options.clear();
        assert_eq!(
            settings.relative_path(),
            Some(RepositoryField::OptionHot("root".to_owned()))
        );
    }

    // A job resolves `${REPO}` before rustic opens it, and only then is the
    // location known (#247). What a reference held is text: a `$` in it
    // starts no reference.
    #[test]
    fn a_resolved_location_that_names_a_folder_here_by_a_relative_path_is_relative() {
        for location in RELATIVE
            .into_iter()
            .map(str::to_owned)
            .chain(RELATIVE.iter().map(|path| format!("local:{path}")))
            .chain(
                [
                    "${HOME}/repo",
                    " /srv/demo-repo",
                    "rclone:backups/repo",
                    "rclone:./repo",
                    "rclone:~/repo",
                    "rclone:repo",
                    "rclone:backups/nas:repo",
                ]
                .map(str::to_owned),
            )
        {
            assert!(is_relative_location(&location), "{location:?}");
        }
    }

    #[test]
    fn a_resolved_location_that_is_a_full_path_or_no_folder_here_is_not_relative() {
        for location in [
            "/srv/demo-repo",
            "/",
            "local:/srv/demo-repo",
            "rclone:nas:backups/repo",
            "rclone:/srv/demo-repo",
            "rest:https://nas.example/repo",
            "opendal:s3",
            "opendal:fs",
            "opendal:sftp:backups",
            // What rustic reads as a backend it doesn't know, and refuses.
            "nas:backups/repo",
            "LOCAL:backups/repo",
            "s3:s3.example.net/demo-backups",
        ] {
            assert!(!is_relative_location(location), "{location:?}");
        }
    }
}
