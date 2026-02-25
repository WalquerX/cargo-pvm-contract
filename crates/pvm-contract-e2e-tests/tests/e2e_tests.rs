//! On-chain E2E tests for examples/example-mytoken.
//!
//! These deploy contracts to anvil-polkadot and verify dispatch routing,
//! error handling, and variant parity through actual blockchain transactions.
//!
//! Requirements: nightly + rust-src + solc + anvil-polkadot + cast
//! Run: cargo test -p pvm-contract-e2e-tests --test e2e_tests -- --ignored --test-threads=1

use pvm_contract_e2e_tests::anvil::shared_anvil;
use pvm_contract_e2e_tests::build::contract;
use pvm_contract_e2e_tests::cast::{CastClient, DEFAULT_ADDRESS, DEFAULT_PRIVATE_KEY};

const ALL_VARIANTS: &[&str] = &[
    "example-mytoken-macro-bump-alloc",
    "example-mytoken-macro-no-sol",
    "example-mytoken-dsl-no-alloc",
];

const DEFAULT_VARIANT: &str = "example-mytoken-macro-bump-alloc";

fn mytoken() -> pvm_contract_e2e_tests::build::Contract {
    contract("example-mytoken")
}

fn deploy_variant(variant: &str) -> (CastClient, String) {
    let c = mytoken();
    c.build();
    let anvil = shared_anvil();
    anvil.reset();
    let cast = CastClient::new(&anvil.rpc_url);
    let hex = c.bytecode_hex(variant, "release");
    let address = cast.deploy(&hex, DEFAULT_PRIVATE_KEY);
    (cast, address)
}

fn deploy_mytoken() -> (CastClient, String) {
    deploy_variant(DEFAULT_VARIANT)
}

// --- Dispatch: generated selector routing works on-chain ---

#[test]
#[ignore] // Requires anvil-polkadot + cast
fn dispatch_routes_view_selectors_correctly() {
    let (cast, address) = deploy_mytoken();

    // totalSupply() and balanceOf(address) should both return 0 on fresh deploy
    let supply = cast.call(&address, "totalSupply()(uint256)", &[]);
    assert_eq!(supply, "0");

    let balance = cast.call(&address, "balanceOf(address)(uint256)", &[DEFAULT_ADDRESS]);
    assert_eq!(balance, "0");
}

#[test]
#[ignore]
fn dispatch_routes_write_selectors_correctly() {
    let (cast, address) = deploy_mytoken();

    // mint and transfer are write selectors — verify they execute and change state
    cast.send(
        &address,
        "mint(address,uint256)",
        &[DEFAULT_ADDRESS, "1000"],
        DEFAULT_PRIVATE_KEY,
    );

    let supply = cast.call(&address, "totalSupply()(uint256)", &[]);
    assert_eq!(supply, "1000", "mint selector didn't update supply");

    let recipient = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
    cast.send(
        &address,
        "transfer(address,uint256)",
        &[recipient, "400"],
        DEFAULT_PRIVATE_KEY,
    );

    let sender_bal = cast.call(&address, "balanceOf(address)(uint256)", &[DEFAULT_ADDRESS]);
    assert_eq!(sender_bal, "600", "transfer selector didn't debit sender");

    let recv_bal = cast.call(&address, "balanceOf(address)(uint256)", &[recipient]);
    assert_eq!(recv_bal, "400", "transfer selector didn't credit recipient");
}

#[test]
#[ignore]
fn dispatch_fallback_handles_unknown_selector() {
    let c = mytoken();
    c.build();
    let anvil = shared_anvil();
    anvil.reset();
    let cast = CastClient::new(&anvil.rpc_url);
    let hex = c.bytecode_hex(DEFAULT_VARIANT, "release");
    let address = cast.deploy(&hex, DEFAULT_PRIVATE_KEY);

    // 0xdeadbeef is not a known selector — fallback should handle it
    let mut cmd = std::process::Command::new("cast");
    cmd.args([
        "send",
        &address,
        "0xdeadbeef",
        "--rpc-url",
        &anvil.rpc_url,
        "--private-key",
        DEFAULT_PRIVATE_KEY,
    ]);

    let output = cmd.output().expect("cast send failed to execute");
    assert!(
        output.status.success(),
        "Fallback should accept unknown selector: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore]
fn dispatch_revert_propagates_on_underflow() {
    let (cast, address) = deploy_mytoken();

    // Transfer with 0 balance — generated dispatch should propagate the revert
    let output = cast.send_expect_revert(
        &address,
        "transfer(address,uint256)",
        &["0x70997970C51812dc3A010C7d01b50e0d17dc79C8", "100"],
        DEFAULT_PRIVATE_KEY,
    );

    assert!(!output.status.success(), "Transfer underflow should revert");
}

// --- Variant parity: all toolchain paths produce equivalent contracts ---

#[test]
#[ignore]
fn all_variants_deploy_and_respond_to_selectors() {
    for variant in ALL_VARIANTS {
        let (cast, address) = deploy_variant(variant);

        let supply = cast.call(&address, "totalSupply()(uint256)", &[]);
        assert_eq!(supply, "0", "{variant}: initial supply should be 0");

        cast.send(
            &address,
            "mint(address,uint256)",
            &[DEFAULT_ADDRESS, "1000"],
            DEFAULT_PRIVATE_KEY,
        );

        let balance = cast.call(&address, "balanceOf(address)(uint256)", &[DEFAULT_ADDRESS]);
        assert_eq!(
            balance, "1000",
            "{variant}: balance after mint should be 1000"
        );
    }
}
