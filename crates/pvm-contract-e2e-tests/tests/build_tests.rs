//! Build artifact tests for examples/example-mytoken.
//!
//! These verify the toolchain produces correct ABI JSON

use pvm_contract_e2e_tests::build::contract;

fn mytoken() -> pvm_contract_e2e_tests::build::Contract {
    contract("example-mytoken")
}

#[test]
#[ignore]
fn build_abi_contains_correct_function_signatures() {
    let c = mytoken();
    c.build();

    let content =
        std::fs::read_to_string(c.abi_json_path("example-mytoken-macro-bump-alloc", "release"))
            .unwrap();
    let abi: serde_json::Value = serde_json::from_str(&content).unwrap();
    let functions: Vec<&serde_json::Value> = abi
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "function")
        .collect();

    assert_eq!(
        functions.len(),
        4,
        "Expected 4 functions, got {}",
        functions.len()
    );

    // Verify each function's signature matches what the toolchain should generate
    let find_fn = |name: &str| -> &serde_json::Value {
        functions
            .iter()
            .find(|f| f["name"] == name)
            .unwrap_or_else(|| panic!("Function '{name}' not found in ABI"))
    };

    // totalSupply() -> uint256, view
    let f = find_fn("totalSupply");
    assert_eq!(f["inputs"].as_array().unwrap().len(), 0);
    assert_eq!(f["outputs"][0]["type"], "uint256");
    assert_eq!(f["stateMutability"], "view");

    // balanceOf(address) -> uint256, view
    let f = find_fn("balanceOf");
    assert_eq!(f["inputs"][0]["type"], "address");
    assert_eq!(f["inputs"][0]["name"], "account");
    assert_eq!(f["outputs"][0]["type"], "uint256");
    assert_eq!(f["stateMutability"], "view");

    // transfer(address to, uint256 amount), nonpayable
    let f = find_fn("transfer");
    assert_eq!(f["inputs"][0]["type"], "address");
    assert_eq!(f["inputs"][0]["name"], "to");
    assert_eq!(f["inputs"][1]["type"], "uint256");
    assert_eq!(f["inputs"][1]["name"], "amount");
    assert_eq!(f["outputs"].as_array().unwrap().len(), 0);
    assert_eq!(f["stateMutability"], "nonpayable");

    // mint(address to, uint256 amount), nonpayable
    let f = find_fn("mint");
    assert_eq!(f["inputs"][0]["type"], "address");
    assert_eq!(f["inputs"][0]["name"], "to");
    assert_eq!(f["inputs"][1]["type"], "uint256");
    assert_eq!(f["inputs"][1]["name"], "amount");
    assert_eq!(f["outputs"].as_array().unwrap().len(), 0);
    assert_eq!(f["stateMutability"], "nonpayable");
}
