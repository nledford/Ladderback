use super::*;
use crate::fleet::Fingerprint;

/// The kind byte is the one decision 120 names, both ways, and no other
/// byte names a kind.
#[test]
fn a_kind_is_its_byte() {
    for kind in [BindingKind::None, BindingKind::TlsExporter] {
        assert_eq!(BindingKind::from_byte(kind.byte()), Some(kind));
    }
    assert_eq!(BindingKind::None.byte(), 0);
    assert_eq!(BindingKind::TlsExporter.byte(), 1);
    for byte in 2..=u8::MAX {
        assert_eq!(BindingKind::from_byte(byte), None, "byte {byte}");
    }
}

/// A pinned token's address is the hub's own TLS, kind 1; a CA token's is
/// a proxy's, kind 0.
#[test]
fn a_token_names_the_kind_of_its_address() {
    let pinned = TokenTls::Pinned(Fingerprint::from_digest([7; KEY_BYTES]));
    assert_eq!(BindingKind::of_token(pinned), BindingKind::TlsExporter);
    assert_eq!(BindingKind::of_token(TokenTls::Ca), BindingKind::None);
}

/// Kind 0 binds to nothing on any session; kind 1 binds to the session's
/// exporter value, and to nothing at all where the session has none.
#[test]
fn a_kind_binds_to_what_the_session_gives() {
    let exporter = [9; KEY_BYTES];
    assert_eq!(BindingKind::None.binding(None), Some(Binding::None));
    assert_eq!(
        BindingKind::None.binding(Some(exporter)),
        Some(Binding::None)
    );
    assert_eq!(
        BindingKind::TlsExporter.binding(Some(exporter)),
        Some(Binding::TlsExporter(exporter))
    );
    assert_eq!(BindingKind::TlsExporter.binding(None), None);
}
