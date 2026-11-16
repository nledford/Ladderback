//! The credentials of the proxy URLs in an environment (technical design
//! §10.2): a worker and a hook are given the daemon's proxy variables as
//! they are, so a program either starts can use them, and whatever their
//! output shows is masked of what those URLs hold as credentials.

use std::ffi::OsString;

use super::{SecretValue, url_credentials};

/// The variables that hold the URL of a proxy, and so may carry credentials
/// in its userinfo, in both the cases programs read. `NO_PROXY` holds host
/// names, not a URL.
pub const PROXY_URL_VARIABLES: [&str; 6] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

/// The credentials in the proxy URLs of `vars`: the userinfo of each (see
/// [`url_credentials`]). They stay in the environment a program is given,
/// where a program it starts uses them, so the daemon can't keep them from
/// it; it masks them wherever its output shows them.
pub fn proxy_credentials(vars: &[(OsString, OsString)]) -> Vec<SecretValue> {
    proxy_credentials_by_variable(vars)
        .flat_map(|(_, credentials)| credentials)
        .collect()
}

/// The credentials of each proxy URL of `vars`, with the variable it is in,
/// in the order of `vars`. A value that is not text is left alone.
pub fn proxy_credentials_by_variable(
    vars: &[(OsString, OsString)],
) -> impl Iterator<Item = (&'static str, Vec<SecretValue>)> + '_ {
    vars.iter().filter_map(|(name, value)| {
        let variable = PROXY_URL_VARIABLES
            .into_iter()
            .find(|proxy| name == proxy)?;
        Some((variable, url_credentials(value.to_str()?)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::Redactor;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    fn masked(pairs: &[(&str, &str)], text: &str) -> String {
        Redactor::new(&proxy_credentials(&vars(pairs))).redact(text)
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

    #[test]
    fn each_proxy_variables_credentials_are_named_with_it_in_order() {
        let base = vars(&[
            ("https_proxy", "http://demo:CANARY-one@proxy.demo:3128"),
            ("PATH", "/usr/bin"),
            ("HTTP_PROXY", "http://proxy.demo:3128"),
        ]);
        let named: Vec<(&str, usize)> = proxy_credentials_by_variable(&base)
            .map(|(variable, credentials)| (variable, credentials.len()))
            .collect();
        assert_eq!(named.first().map(|(name, _)| *name), Some("https_proxy"));
        assert!(named.first().is_some_and(|(_, count)| *count > 0));
        assert_eq!(named.get(1), Some(&("HTTP_PROXY", 0)));
        assert_eq!(named.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_proxy_value_that_is_not_text_is_left_alone() {
        use std::os::unix::ffi::OsStringExt;
        let base = vec![(
            OsString::from("HTTPS_PROXY"),
            OsString::from_vec(b"http://demo:\xff@proxy.demo".to_vec()),
        )];
        assert!(proxy_credentials(&base).is_empty());
    }
}
