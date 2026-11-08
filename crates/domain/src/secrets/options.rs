//! Which backend options hold a credential.

/// Whether a backend option of this name holds a credential, whoever wrote
/// it. The values of such options are secrets even when they are written
/// inline in the config, so the redactor hides them (I4, §10.2).
///
/// A name is a secret when it has `password`, `passwd`, `secret`, `token`,
/// `credential`, `passphrase` or `delegation` in it or ends in `_key`; `-`
/// and `_` are the same, and case doesn't matter. Names that only identify a
/// credential (`access_key_id`, `application_key_id`) and `key`, which SFTP
/// uses for the path of a key file, are not secrets.
pub fn is_secret_option(_name: &str) -> bool {
    todo!("whether the option's name says it holds a credential")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_credentials_of_the_backends_rustic_uses_are_secrets() {
        for name in [
            // WebDAV, FTP, pCloud
            "password",
            // S3
            "secret_access_key",
            "session_token",
            "server_side_encryption_customer_key",
            // B2
            "application_key",
            // Azure
            "account_key",
            "sas_token",
            "encryption_key",
            "client_secret",
            // GCS
            "credential",
            "token",
            // Tencent COS, Alibaba OSS
            "secret_key",
            "access_key_secret",
            "security_token",
            // Dropbox, Google Drive, OneDrive
            "access_token",
            "refresh_token",
            // Swift
            "temp_url_key",
            // GitHub Actions cache
            "runtime_token",
            // Other spellings
            "passwd",
            "passphrase",
            "key_passphrase",
            "private_key",
            "delegation",
        ] {
            assert!(is_secret_option(name), "{name}");
        }
    }

    #[test]
    fn names_that_do_not_hold_a_secret_are_not() {
        for name in [
            "bucket",
            "endpoint",
            "region",
            "root",
            "container",
            "account_name",
            "user",
            "username",
            // They identify a credential, they are not the secret half.
            "access_key_id",
            "application_key_id",
            "key_id",
            // SFTP's `key` is the path of a key file.
            "key",
            // rustic's own options.
            "retry",
            "connections",
            "throttle",
            "rclone-command",
            "rest-url",
            "use-password",
            "use_password",
            "",
        ] {
            assert!(!is_secret_option(name), "{name:?}");
        }
    }

    #[test]
    fn case_and_hyphens_do_not_matter() {
        for name in [
            "Password",
            "SECRET_ACCESS_KEY",
            "secret-access-key",
            "Application-Key",
            "SAS-TOKEN",
        ] {
            assert!(is_secret_option(name), "{name}");
        }
        assert!(!is_secret_option("USE-PASSWORD"));
        assert!(!is_secret_option("Application-Key-Id"));
    }
}
