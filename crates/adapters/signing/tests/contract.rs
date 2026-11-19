//! Ed25519 over `aws-lc-rs` meets the `Signing` contract the fake meets.

use app::contract;
use signing_adapter::AwsLcSigning;

#[test]
fn aws_lc_signing_meets_the_signing_contract() {
    contract::signing::run(&AwsLcSigning);
}
