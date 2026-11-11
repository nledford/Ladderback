//! Which backend options hold a credential.

/// A name that has any of these in it holds a secret: `password`,
/// `secret_access_key`, `refresh_token`, `credential` (GCS's service account),
/// `passphrase`, `delegation` (WebHDFS's delegation token).
const SECRET_WORDS: [&str; 7] = [
    "password",
    "passwd",
    "secret",
    "token",
    "credential",
    "passphrase",
    "delegation",
];

/// Options that name `password` or the like without holding one. rustic's
/// rclone backend reads `use-password` as a switch.
const NOT_SECRETS: [&str; 1] = ["use_password"];

/// Names that hold a credential only as the whole name. rclone's SFTP, FTP
/// and SMB backends keep their password in `pass`; `bypass_cache` and
/// `passive` hold none.
const SECRET_NAMES: [&str; 1] = ["pass"];

/// Whether a backend option of this name holds a credential, whoever wrote
/// it. The values of such options are secrets even when they are written
/// inline in the config, so the redactor hides them (I4, §10.2).
///
/// A name is a secret when it is one of [`SECRET_NAMES`], has a
/// [`SECRET_WORDS`] word in it or ends in `_key` (`account_key`,
/// `application_key`, `encryption_key`, `temp_url_key`); `-` and `_` are the
/// same, and case doesn't matter.
/// Names that only identify a credential (`access_key_id`,
/// `application_key_id`) and `key`, which SFTP uses for the path of a key
/// file, are not secrets. Anything whose value comes from a `${…}` reference
/// is a secret whatever its name.
pub fn is_secret_option(name: &str) -> bool {
    let name = name.to_ascii_lowercase().replace('-', "_");
    if NOT_SECRETS.contains(&name.as_str()) {
        return false;
    }
    SECRET_NAMES.contains(&name.as_str())
        || SECRET_WORDS.iter().any(|word| name.contains(word))
        || name.ends_with("_key")
}

/// Whether a setting of this name in an rclone connection string holds a
/// credential.
pub(super) fn is_rclone_secret_option(name: &str) -> bool {
    let _ = name;
    todo!()
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

    #[test]
    fn pass_is_a_secret_only_as_the_whole_name() {
        // rclone's SFTP, FTP and SMB backends keep their password in `pass`.
        for name in ["pass", "PASS", "Pass"] {
            assert!(is_secret_option(name), "{name}");
        }
        for name in [
            "bypass_cache",
            "bypass-cache",
            "passive",
            "pass_through",
            "compass",
        ] {
            assert!(!is_secret_option(name), "{name}");
        }
    }

    #[test]
    fn rclones_own_credential_names_are_secrets_as_whole_names() {
        for name in [
            "key_file_pass",
            "key-file-pass",
            "KEY_PEM",
            "sas_url",
            "connection_string",
            "sse_customer_key_base64",
            // Storj's access grant, Filen's master keys, SugarSync's
            // authorization, the headers of WebDAV and HTTP (which take
            // `Authorization,Bearer …`), and Proton Drive's one-time code.
            "access_grant",
            "Access-Grant",
            "master_keys",
            "authorization",
            "headers",
            "2fa",
        ] {
            assert!(is_secret_option(name), "{name}");
        }
        for name in [
            "key_file",
            "key_use_agent",
            "pubkey",
            "sse_customer_key_md5",
            "url",
            "access",
            "master_key_id",
            "header",
        ] {
            assert!(!is_secret_option(name), "{name}");
        }
    }

    #[test]
    fn key_is_a_credential_only_in_an_rclone_connection_string() {
        // rclone's B2, Swift and Azure Blob backends keep a credential in
        // `key`; a rustic option of that name is SFTP's key file path.
        for name in ["key", "KEY", "pass", "secret_access_key", "key_pem"] {
            assert!(is_rclone_secret_option(name), "{name}");
        }
        assert!(!is_secret_option("key"));
        for name in ["key_file", "access_key_id", "use_password", "host", ""] {
            assert!(!is_rclone_secret_option(name), "{name}");
        }
    }
}
