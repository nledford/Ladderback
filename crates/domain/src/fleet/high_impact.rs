//! Who may take a high-impact action on the hub's web page: make a pairing
//! token, approve an agent or re-key one (§16, decision 120, "Who may make a
//! token, approve or re-key").

/// Whether a high-impact request must be refused until sign-in is on
/// (`pairing-needs-sign-in`): it came from beyond loopback while sign-in by
/// the admin password is off, so there is no password to enter again and
/// the key ID that would stand in for it is public. On loopback with
/// sign-in off the typed key ID is the step-up; with sign-in on, the admin
/// password is, wherever the request came from.
pub fn needs_sign_in(password_sign_in: bool, beyond_loopback: bool) -> bool {
    let _ = (password_sign_in, beyond_loopback);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_request_from_beyond_loopback_with_sign_in_off_needs_sign_in() {
        assert!(needs_sign_in(false, true));
        assert!(!needs_sign_in(false, false));
        assert!(!needs_sign_in(true, true));
        assert!(!needs_sign_in(true, false));
    }
}
