//! Build artifact tests for examples/example-mytoken.
//!
//! These verify the toolchain produces correct ABI JSON

use pvm_contract_e2e_tests::build::contract;

fn mytoken() -> pvm_contract_e2e_tests::build::Contract {
    contract("example-mytoken")
}

#[test]
#[ignore]
fn build_produces_expected_abi() {
    let c = mytoken();
    c.build();

    let actual =
        std::fs::read_to_string(c.abi_json_path("example-mytoken-macro-bump-alloc", "release"))
            .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();

    let expected: serde_json::Value =
        serde_json::from_str(include_str!("expected_mytoken_abi.json")).unwrap();

    assert_eq!(actual, expected, "ABI mismatch: check tests/expected_mytoken_abi.json");
}
