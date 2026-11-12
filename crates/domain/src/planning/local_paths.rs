//! The paths on this machine that a repository's settings name. rustic, the
//! programs it starts, the hooks and the password command run in the
//! daemon's working directory (`/` under launchd or a system unit, the home
//! folder under a user unit), never the folder `config.toml` is in, so a
//! relative path there would name a different file, or none, depending on
//! how the daemon was started.
//!
//! A field is judged by what is known of it before a job resolves its
//! `${…}` references: the text up to the first one. A location a job has
//! resolved is judged whole, by [`is_relative_location`].

mod rclone;

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
pub fn is_relative_location(location: &str) -> bool {
    read_location(location, true).is_some_and(|location| location.relative)
}

/// Whether a program is named by a relative path: with a `/` in it, and not
/// at its start. A name without one is looked up on `PATH`.
pub(crate) fn is_relative_program(program: &str) -> bool {
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
    read_location(&head, whole)
}

/// [`location`], from `head`, what is known of it, and whether that is all.
fn read_location(head: &str, whole: bool) -> Option<Location> {
    let Some((scheme, rest)) = head.split_once(':') else {
        // A `/` before any `:` leaves no scheme a reference could finish.
        return (whole || head.contains('/')).then(|| Location {
            backend: Backend::Local,
            relative: starts_relative(head, whole),
        });
    };
    let (backend, relative) = match scheme {
        "local" => (Backend::Local, starts_relative(rest, whole)),
        "rclone" => (Backend::Rclone, rclone::location_relative(rest, whole)),
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
/// or, for rclone, has a flag that names a file, a folder or a program by
/// one ([`rclone::flags_relative`]). A command with a reference in it is
/// split only once the job resolves it, and one that can't be split is
/// rustic's to refuse.
fn runs_relative(value: &str, starts_rclone: bool) -> bool {
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
    is_relative_program(command.program())
        || (starts_rclone && rclone::flags_relative(command.args()))
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
            // rclone's on-the-fly backends other than `local` reach a remote.
            "rclone::sftp:backups",
            "rclone::s3,provider=AWS,env_auth:bucket/repo",
            "rclone::sftp,host=nas.example.net,description='a/b:c,d':backups",
            "rclone::local:/srv/demo-repo",
            "rclone::local,nounc=true:/srv/demo-repo",
            "rclone::local:${NAME}",
            // Files a setting names by a full path, or one rclone expands.
            "rclone::sftp,host=nas.example.net,key_file=/srv/keys/id:backups",
            "rclone::sftp,host=nas.example.net,key_file=~/.ssh/id:backups",
            "rclone::sftp,host=nas.example.net,key_file=:backups",
            "rclone:nas,override.ca_cert=/etc/ssl/ca.pem:backups",
            // A location written in a setting that is a remote or a full path.
            "rclone::alias,remote=\"nas:backups\":repo",
            "rclone::alias,remote='nas:backups':repo",
            "rclone::crypt,remote=':sftp,host=nas.example.net:backups':",
            "rclone::alias,remote=/srv/demo-repo:",
            "rclone::union,upstreams='/srv/a nas:b:ro':",
            "rclone::union,upstreams='\"/srv/my backups\" nas:b':",
            "rclone::union,upstreams=:",
            "rclone::combine,upstreams='a=nas:ro b=/srv/b':",
            // A remote whose settings make it of a backend other than `local`,
            // a setting a global flag reads that names no file, and a path
            // that only looks like settings.
            "rclone:nas,type=sftp:backups",
            "rclone:nas,type=local,type=sftp:backups",
            "rclone:nas,override.suffix=old:backups",
            "rclone:nas:key_file=keys/id:backups",
            // What rclone refuses: a backend name with a `/`, an empty name, a
            // setting's name with a character a name can't hold.
            "rclone::local/x:backups",
            "rclone:,x:backups",
            "rclone:,key_file=keys/id:backups",
            "rclone::local,b@d=x:backups",
        ] {
            assert_eq!(at(location).relative_path(), None, "{location:?}");
            let mut hot = at("/srv/demo-repo");
            hot.repo_hot = Some(location.to_owned());
            assert_eq!(hot.relative_path(), None, "{location:?}");
        }
    }

    #[test]
    fn a_remote_whose_settings_hold_a_slash_is_not_a_folder_here() {
        // A `/` in a setting of a remote of rclone's configuration file, before
        // the `:` that ends the remote's name, is part of the setting: the
        // remote reaches no folder on this machine.
        for location in [
            "rclone:nas,root_folder_id=a/b:repo",
            "rclone:nas,shared_with_me:backups/repo",
            "rclone:nas,description='a/b':repo",
        ] {
            assert_eq!(at(location).relative_path(), None, "{location:?}");
            assert!(!is_relative_location(location), "{location:?}");
        }
    }

    /// rclone locations that name a file or folder on this machine by a
    /// relative path, other than as a folder with no remote: the path of an
    /// on-the-fly `local` backend, a setting of a connection string that
    /// names a file, and a location written in a setting.
    const RCLONE_RELATIVE: [&str; 26] = [
        // rclone's `local` backend serves its path from the working directory.
        "rclone::local:backups",
        "rclone::local:",
        "rclone::local:./repo",
        "rclone::local:~/repo",
        "rclone::local:nas:repo",
        "rclone::local,nounc=true:backups",
        "rclone::local,links:backups",
        "rclone::local,description='a:b,c''d':backups",
        // A `\` before any `:` leaves no remote, as a `/` does.
        "rclone:nas\\x:repo",
        // A remote its settings make a `local` one: rclone reads its type from
        // them before its configuration file, the last when there are two.
        "rclone:nas,type=local:backups",
        "rclone:nas,type=local:",
        "rclone:nas,type=sftp,type=local:backups",
        "rclone::crypt,remote='nas,type=local:backups':",
        // A setting that names a file here.
        "rclone::sftp,host=nas.example.net,key_file=keys/id:backups",
        "rclone::sftp,host=nas.example.net,known_hosts_file='hosts':backups",
        "rclone::sftp,host=nas.example.net,ssh='bin/ssh -i id':backups",
        "rclone:nas,service_account_file=sa.json:backups",
        "rclone:nas,override.ca_cert=ca.pem:backups",
        "rclone:nas,global.log_file=rclone.log:backups",
        // A location written in a setting, at any level.
        "rclone::alias,remote=backups:repo",
        "rclone::crypt,remote=':local:backups',password=demo:",
        "rclone::crypt,remote=':alias,remote=''backups'':':",
        "rclone::union,upstreams='/srv/a backups:ro':",
        "rclone::union,upstreams='/srv/a \"b c\"':",
        "rclone::union,upstreams='\"/srv/a\" backups':",
        "rclone::combine,upstreams='a=/srv/a b=backups':",
    ];

    #[test]
    fn an_rclone_location_that_names_a_path_here_by_a_relative_one_is_found() {
        for location in RCLONE_RELATIVE
            .into_iter()
            .map(str::to_owned)
            .chain(["rclone::local:backups/${NAME}".to_owned()])
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
        // A job reads a location a reference supplied the same way.
        for location in RCLONE_RELATIVE {
            assert!(is_relative_location(location), "{location:?}");
        }
    }

    #[test]
    fn a_location_written_in_a_setting_is_read_four_levels_deep_and_found_deeper() {
        // `levels` locations, each written in the `remote` of the one before,
        // the last a full path.
        let nested = |levels: usize| {
            let mut location = "/srv/demo-repo".to_owned();
            for _ in 1..levels {
                location = format!(":alias,remote='{}':", location.replace('\'', "''"));
            }
            format!("rclone:{location}")
        };
        assert!(!is_relative_location(&nested(4)), "{:?}", nested(4));
        assert_eq!(at(&nested(4)).relative_path(), None);
        assert!(is_relative_location(&nested(5)), "{:?}", nested(5));
        assert_eq!(
            at(&nested(5)).relative_path(),
            Some(RepositoryField::Repository)
        );
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
    fn an_rclone_flag_that_names_a_file_here_by_a_relative_path_is_found() {
        let rclone = |command: &str| {
            with_option("rclone:nas:backups", "rclone-command", command).relative_path()
        };
        for flags in [
            "--log-file rclone.log",
            "--log-file=logs/rclone.log",
            "--cert tls/cert.pem",
            "--key=tls/key.pem",
            "--client-ca ca.pem",
            "--htpasswd htpasswd",
            "--ca-cert /etc/ssl/ca.pem --ca-cert ca.pem",
            "--client-cert client.pem",
            "--client-key=client.key",
            "--cache-dir cache",
            "--temp-dir=tmp",
            "--exclude-from excludes.txt",
            "--rc-htpasswd ~/htpasswd",
            "--metrics-template=page.html",
            // A program named by a relative path, as the first of the words.
            "--password-command './get-pass --vault demo'",
            "--password-command=bin/get-pass",
            "--sftp-ssh '\"bin/my ssh\" -i id'",
            "--sftp-ssh '\"\"\"/usr/bin/ssh\"'",
            "--webdav-bearer-token-command 'bin/token'",
            // A backend's file, or a location or upstream that is a folder here.
            "--sftp-key-file keys/id",
            "--s3-shared-credentials-file=aws/credentials",
            "--smb-kerberos-ccache FILE:krb5cc",
            "--alias-remote backups",
            "--crypt-remote :local:backups",
            "--backup-dir=old",
            "--union-upstreams 'nas:a backups:ro'",
            "--combine-upstreams 'a=nas:a b=backups'",
            // A `--` ends nothing: pflag gives it to a flag before it that
            // takes a value, and which of rclone's flags take one isn't known.
            "--user-agent -- --log-file rclone.log",
            "-- --log-file rclone.log",
            // A server that listens on a socket named by a relative path.
            "--addr unix://rclone.sock",
        ] {
            assert_eq!(
                rclone(&format!("rclone serve restic {flags}")),
                option("rclone-command"),
                "{flags:?}"
            );
        }
        for flags in [
            "--log-file /var/log/rclone.log --cert=/etc/tls/cert.pem",
            "--ca-cert /etc/ssl/ca.pem --client-ca=/etc/ssl/clients.pem",
            // Not set, or standard input.
            "--log-file= --cache-dir=",
            "--exclude-from -",
            // A program looked up on `PATH`, or named in full.
            "--password-command 'pass show rclone'",
            "--sftp-ssh '/usr/bin/ssh -i id'",
            "--sftp-ssh '\"/usr/bin/ssh\" -i id'",
            // A list rclone refuses, so runs nothing: a `"` inside a word, a
            // word that goes on after its closing `"`, a `"` never closed.
            "--sftp-ssh 'bin/s\"sh'",
            "--sftp-ssh '\"bin/ssh\"x'",
            "--sftp-ssh '\"bin/ssh'",
            // rclone expands a leading `~` or variable in these itself.
            "--sftp-key-file ~/.ssh/id",
            "--drive-service-account-file=$HOME/sa.json",
            // A remote, or a server that listens on an address or a full path.
            "--alias-remote nas:backups",
            "--crypt-remote=:sftp,host=nas.example.net:backups",
            "--union-upstreams 'nas:a /srv/b:ro'",
            "--combine-upstreams 'a=nas:ro'",
            "--addr localhost:0",
            "--rc-addr unix:///run/rclone.sock",
            "--smb-kerberos-ccache DIR:/run/krb5",
            // A flag that names no file.
            "--verbose --log-level INFO --user-agent rclone.log",
            "--sftp-host nas.example.net",
        ] {
            assert_eq!(
                rclone(&format!("rclone serve restic {flags}")),
                None,
                "{flags:?}"
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
            "rclone::sftp,host=nas.example.net:backups",
            "rclone::local:/srv/demo-repo",
        ] {
            assert!(!is_relative_location(location), "{location:?}");
        }
    }
}
