//! The paths on this machine that a repository's settings name. rustic, the
//! programs it starts and the hooks run in the daemon's working directory
//! (`/` under launchd or a system unit, the home folder under a user unit),
//! never the folder `config.toml` is in, so a relative path there would name
//! a different file, or none, depending on how the daemon was started.

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
        todo!()
    }
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
}
