//! The environment a worker starts with (invariant I4: workers get a
//! scrubbed environment).
//!
//! The daemon's own environment may hold credentials: `AWS_*`, `AZURE_*` and
//! `GOOGLE_*` for the cloud SDKs, `RUSTIC_PASSWORD`, `RCLONE_CONFIG_PASS`,
//! and whatever variable a secret reference was read from. A worker is given
//! the secrets of its job over stdin, so none of those is of any use to it,
//! and a program it starts (rclone, ssh) would pass them on in turn.

use std::ffi::OsString;

use domain::secrets::SecretValue;

/// The variables a worker keeps, by exact name (case matters). Only what the
/// worker or the programs it starts (rclone, ssh) need, and that names a
/// place or a setting: where the programs and the temporary files are, who
/// the user is, the locale and time zone, the agent socket, the certificate
/// locations, rclone's config file, and the proxies. None of these is a
/// credential. A proxy URL that embeds credentials is the user's choice and
/// passes as it is, but it is masked wherever the worker's output shows it:
/// see [`proxy_credentials`].
const KEPT: [&str; 24] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "TZ",
    "LANG",
    "LC_ALL",
    "SSH_AUTH_SOCK",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "RCLONE_CONFIG",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

/// What of `base` a worker may start with: the variables of [`KEPT`], in the
/// order given, and nothing else.
pub(crate) fn scrubbed(
    base: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    base.into_iter()
        .filter(|(name, _)| KEPT.iter().any(|kept| name == kept))
        .collect()
}

/// The variables that hold the URL of a proxy, and so may carry credentials
/// in its userinfo. `NO_PROXY` holds host names, not a URL.
const PROXY_URLS: [&str; 6] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

/// The credentials in the proxy URLs of `vars`: the userinfo of each (see
/// [`domain::secrets::url_credentials`]). They stay in the worker's
/// environment, where a program it starts uses them, so the daemon can't
/// keep them from the worker; it masks them wherever its output shows them.
pub(crate) fn proxy_credentials(vars: &[(OsString, OsString)]) -> Vec<SecretValue> {
    let _ = vars;
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    fn names(vars: &[(OsString, OsString)]) -> Vec<String> {
        vars.iter()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn what_a_worker_and_the_programs_it_starts_need_is_kept_in_the_order_given() {
        let base = vars(&[
            ("LANG", "en_US.UTF-8"),
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/demo"),
            ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
            ("TMPDIR", "/tmp"),
            ("RCLONE_CONFIG", "/home/demo/rclone.conf"),
            ("HTTPS_PROXY", "http://proxy.demo:3128"),
            ("no_proxy", "localhost"),
        ]);
        assert_eq!(scrubbed(base.clone()), base);
    }

    #[test]
    fn variables_that_hold_or_point_at_credentials_are_dropped() {
        let base = vars(&[
            ("PATH", "/usr/bin"),
            ("AWS_SECRET_ACCESS_KEY", "CANARY-aws"),
            ("AWS_ACCESS_KEY_ID", "CANARY-id"),
            ("AZURE_STORAGE_KEY", "CANARY-azure"),
            ("GOOGLE_APPLICATION_CREDENTIALS", "/home/demo/key.json"),
            ("RUSTIC_PASSWORD", "CANARY-rustic"),
            ("RUSTIC_PASSWORD_FILE", "/home/demo/pw"),
            ("RCLONE_CONFIG_PASS", "CANARY-rclone"),
            ("DEMO_B2_KEY", "CANARY-b2"),
        ]);
        assert_eq!(names(&scrubbed(base)), ["PATH"]);
    }

    #[test]
    fn a_name_that_only_starts_like_an_allowed_one_is_dropped() {
        let base = vars(&[
            ("PATHOLOGICAL", "x"),
            ("HOMEBREW_GITHUB_API_TOKEN", "CANARY-gh"),
            ("TZX", "x"),
            ("lang", "x"),
            ("Path", "x"),
            ("HOME_SECRET", "x"),
            ("HTTP_PROXY_PASSWORD", "x"),
            ("USER", "demo"),
        ]);
        assert_eq!(names(&scrubbed(base)), ["USER"]);
    }

    #[test]
    fn a_value_that_looks_like_a_name_does_not_keep_its_variable() {
        let base = vars(&[("DEMO_NOTE", "PATH"), ("PATH", "AWS_SECRET_ACCESS_KEY")]);
        assert_eq!(scrubbed(base), vars(&[("PATH", "AWS_SECRET_ACCESS_KEY")]));
    }

    #[test]
    fn nothing_in_gives_nothing_out() {
        assert!(scrubbed(Vec::new()).is_empty());
    }

    fn masked(pairs: &[(&str, &str)], text: &str) -> String {
        domain::secrets::Redactor::new(&proxy_credentials(&vars(pairs))).redact(text)
    }

    #[test]
    fn the_credentials_of_every_proxy_variable_are_masked_whatever_its_case() {
        let base = [
            ("HTTP_PROXY", "http://demo:CANARY-one@proxy.demo:3128"),
            ("HTTPS_PROXY", "http://demo:CANARY-two@proxy.demo:3128"),
            ("ALL_PROXY", "socks5://demo:CANARY-three@proxy.demo:1080"),
            ("http_proxy", "http://demo:CANARY-four@proxy.demo:3128"),
            ("https_proxy", "http://CANARY-five@proxy.demo:3128"),
            ("all_proxy", "demo:CANARY-six@proxy.demo:1080"),
        ];
        for secret in [
            "CANARY-one",
            "CANARY-two",
            "CANARY-three",
            "CANARY-four",
            "CANARY-five",
            "CANARY-six",
        ] {
            assert_eq!(
                masked(&base, &format!("407 {secret} refused")),
                "407 *** refused",
                "{secret} shows"
            );
        }
    }

    #[test]
    fn only_what_a_proxy_url_holds_as_credentials_is_masked() {
        let base = [
            ("HTTPS_PROXY", "http://demo:CANARY-one@proxy.demo:3128"),
            ("http_proxy", "http://proxy.demo:3128"),
            // Hosts, not a URL.
            ("NO_PROXY", "localhost,demo:CANARY-hosts@internal"),
            ("no_proxy", "demo:CANARY-hosts@internal"),
            // Not a proxy variable.
            ("DEMO_URL", "http://demo:CANARY-other@somewhere.demo"),
        ];
        assert_eq!(
            masked(
                &base,
                "via proxy.demo:3128 CANARY-hosts CANARY-other CANARY-one"
            ),
            "via proxy.demo:3128 CANARY-hosts CANARY-other ***"
        );
    }
}
