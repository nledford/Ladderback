//! ECDSA P-256 keys through `rcgen` meet the `TlsKeys` contract the fake
//! meets.

use app::contract;
use tls_adapter::RcgenTlsKeys;

#[test]
fn rcgen_tls_keys_meet_the_tls_keys_contract() {
    contract::tls_keys::run(&RcgenTlsKeys);
}
