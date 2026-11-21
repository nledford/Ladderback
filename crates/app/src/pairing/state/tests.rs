use std::sync::Arc;

use api::{PairedHubDto, PairingDto, TokenCertificateDto};
use domain::fleet::{Fingerprint, HubAddress, HubPin, PublicKey, TokenTls};

use super::*;
use crate::fakes::InMemoryIdentity;

fn key(byte: u8) -> PublicKey {
    PublicKey::from_bytes([byte; 32]).expect("a public key")
}

fn pin(tls: TokenTls) -> HubPin {
    HubPin::new(
        key(7),
        tls,
        key(9),
        HubAddress::parse("https://hub.example:9898").expect("an address"),
    )
}

async fn state_of(pin: Option<HubPin>) -> PairingState {
    let identity = Arc::new(InMemoryIdentity::default());
    if let Some(pin) = pin {
        identity.write_pin(&pin).await.expect("the pin is kept");
    }
    PairingState::new(identity)
}

#[tokio::test]
async fn a_machine_without_a_pin_isnt_paired() {
    let read = state_of(None).await.read().await.expect("an answer");
    assert_eq!(
        read,
        PairingDto {
            paired: false,
            hub: None,
        }
    );
}

#[tokio::test]
async fn a_pinned_hub_is_named_by_its_address_and_the_fingerprint_of_its_key() {
    let read = state_of(Some(pin(TokenTls::Ca)))
        .await
        .read()
        .await
        .expect("an answer");
    assert_eq!(
        read,
        PairingDto {
            paired: true,
            hub: Some(PairedHubDto {
                address: "https://hub.example:9898".to_owned(),
                fingerprint: Fingerprint::of_key(&key(7)).to_string(),
                certificate: TokenCertificateDto::Ca,
            }),
        }
    );
}

#[tokio::test]
async fn a_hub_whose_own_certificate_is_pinned_says_so() {
    let tls = TokenTls::Pinned(Fingerprint::of_key(&key(3)));
    let read = state_of(Some(pin(tls)))
        .await
        .read()
        .await
        .expect("an answer");
    let hub = read.hub.expect("a hub");
    assert_eq!(hub.certificate, TokenCertificateDto::Pinned);
}
