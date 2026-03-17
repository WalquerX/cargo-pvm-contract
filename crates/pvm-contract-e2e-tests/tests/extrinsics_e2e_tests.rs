//! E2E tests for `cargo-pvm-contract-extrinsics` — the Substrate extrinsics path.
//!
//! These tests exercise the native Substrate RPC (subxt) against `revive-dev-node`,
//! proving that upload, instantiate, call, remove, and query operations work
//! end-to-end through `pallet-revive` dispatchables.
//!
//! Each test starts its own `revive-dev-node` instance for full isolation.
//!
//! Requirements: nightly + rust-src + solc + revive-dev-node
//! Run: cargo test -p pvm-contract-e2e-tests --test extrinsics_e2e_tests -- --test-threads=1

use cargo_pvm_contract_extrinsics::Code;
use pvm_contract_e2e_tests::build::contract;
use pvm_contract_e2e_tests::dev_node::SubstrateDevNode;
use pvm_contract_e2e_tests::substrate_client::{SubstrateClient, encode_call};

const DEFAULT_VARIANT: &str = "example-mytoken-macro-bump-alloc";

fn mytoken() -> pvm_contract_e2e_tests::build::Contract {
    contract("example-mytoken")
}

fn build_mytoken() -> (Vec<u8>, std::path::PathBuf) {
    let c = mytoken();
    c.build();
    let bytecode =
        std::fs::read(c.polkavm_binary(DEFAULT_VARIANT, "release")).expect("read polkavm binary");
    let abi_path = c.abi_json_path(DEFAULT_VARIANT, "release");
    (bytecode, abi_path)
}

fn start_and_client() -> (SubstrateDevNode, SubstrateClient) {
    let node = SubstrateDevNode::start();
    let client = SubstrateClient::new(node.ws_url());
    (node, client)
}

#[tokio::test]
async fn instantiate_dry_run_estimates_gas() {
    let (_node, client) = start_and_client();
    let alice = SubstrateClient::alice();
    let (bytecode, _) = build_mytoken();

    let result = client
        .instantiate_dry_run(Code::Upload(bytecode), vec![], &alice)
        .await
        .expect("instantiate dry-run");

    assert!(
        result.gas_required.ref_time() > 0,
        "gas_required ref_time should be > 0"
    );
    assert!(result.result.is_ok(), "dry-run should succeed");
}

#[tokio::test]
async fn call_revert_detected() {
    let (_node, client) = start_and_client();
    let alice = SubstrateClient::alice();
    let (bytecode, abi_path) = build_mytoken();

    let deploy = client
        .instantiate(Code::Upload(bytecode), vec![], &alice)
        .await
        .expect("deploy");

    // Transfer with 0 balance — should revert
    let transfer_data = encode_call(
        &abi_path,
        "transfer",
        &["0x0000000000000000000000000000000000000001", "100"],
    );
    let result = client
        .call_dry_run(deploy.contract_address, transfer_data, &alice)
        .await
        .expect("transfer dry-run");

    match result.result {
        Ok(ref exec_result) => {
            assert!(
                exec_result.did_revert(),
                "transfer with 0 balance should revert"
            );
        }
        Err(_) => {
            // A dispatch error also indicates failure, which is acceptable
        }
    }
}

#[tokio::test]
async fn remove_code_after_upload() {
    let (_node, client) = start_and_client();
    let alice = SubstrateClient::alice();
    let (bytecode, _) = build_mytoken();

    client.upload_code(&bytecode, &alice).await.expect("upload");

    let code_hash = cargo_pvm_contract_extrinsics::ContractBinary(bytecode).code_hash();
    client
        .remove_code(subxt::utils::H256::from(code_hash), &alice)
        .await
        .expect("remove_code should succeed when no instances exist");
}

#[tokio::test]
async fn full_lifecycle_upload_instantiate_call() {
    let (_node, client) = start_and_client();
    let alice = SubstrateClient::alice();
    let (bytecode, abi_path) = build_mytoken();

    // 1. Map alice's account (may already be mapped in dev mode)
    let alice_addr = match client.map_account(&alice).await {
        Ok(alice_map) => {
            assert_ne!(alice_map.address, sp_core::H160::zero());
            format!("{:?}", alice_map.address)
        }
        Err(e) if e.to_string().contains("AccountAlreadyMapped") => {
            // In dev mode Alice is pre-mapped; derive her EVM address
            let alice_id = subxt_signer::sr25519::dev::alice().public_key().0;
            let addr = cargo_pvm_contract_extrinsics::AccountIdMapper::to_address(&alice_id);
            format!("{:?}", addr)
        }
        Err(e) => panic!("map_account: {e}"),
    };

    // 2. Deploy contract (upload + instantiate in one shot)
    let deploy = client
        .instantiate(Code::Upload(bytecode), vec![], &alice)
        .await
        .expect("instantiate");
    assert_ne!(deploy.contract_address, sp_core::H160::zero());

    // 3. Verify totalSupply is 0
    let supply_data = encode_call(&abi_path, "totalSupply", &[]);
    let supply_result = client
        .call_dry_run(deploy.contract_address, supply_data, &alice)
        .await
        .expect("totalSupply");
    let supply = uint256_from_bytes(&supply_result.result.unwrap().data);
    assert_eq!(supply, 0);

    // 4. Mint 500 tokens
    let mint_data = encode_call(&abi_path, "mint", &[&alice_addr, "500"]);
    client
        .call(deploy.contract_address, mint_data, &alice)
        .await
        .expect("mint");

    // 5. Verify totalSupply is 500
    let supply_data = encode_call(&abi_path, "totalSupply", &[]);
    let supply_result = client
        .call_dry_run(deploy.contract_address, supply_data, &alice)
        .await
        .expect("totalSupply after mint");
    let supply = uint256_from_bytes(&supply_result.result.unwrap().data);
    assert_eq!(supply, 500);

    // 6. Verify balanceOf(alice) is 500
    let balance_data = encode_call(&abi_path, "balanceOf", &[&alice_addr]);
    let balance_result = client
        .call_dry_run(deploy.contract_address, balance_data, &alice)
        .await
        .expect("balanceOf");
    let balance = uint256_from_bytes(&balance_result.result.unwrap().data);
    assert_eq!(balance, 500);
}

/// Decode a big-endian uint256 (32 bytes) into a u128.
fn uint256_from_bytes(data: &[u8]) -> u128 {
    assert!(data.len() >= 32, "uint256 needs at least 32 bytes");
    for &b in &data[..16] {
        assert_eq!(b, 0, "value exceeds u128::MAX");
    }
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&data[16..32]);
    u128::from_be_bytes(buf)
}
