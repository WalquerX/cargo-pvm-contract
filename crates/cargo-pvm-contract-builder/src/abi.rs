use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::Path, process::Command};
use toml_edit::DocumentMut;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AbiJson(Vec<AbiItem>);

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AbiItem {
    Function {
        name: String,
        inputs: Vec<AbiParam>,
        outputs: Vec<AbiParam>,
        #[serde(rename = "stateMutability")]
        #[serde(skip_serializing_if = "Option::is_none")]
        state_mutability: Option<String>,
    },
    Constructor {
        inputs: Vec<AbiParam>,
        #[serde(rename = "stateMutability")]
        #[serde(skip_serializing_if = "Option::is_none")]
        state_mutability: Option<String>,
    },
    Error {
        name: String,
        inputs: Vec<AbiParam>,
    },
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AbiParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
}

pub fn generate_abi_for_bin(manifest_dir: &Path, bin_name: &str) -> Result<Option<AbiJson>> {
    generate_abi_via_feature(manifest_dir, bin_name)
}

fn generate_abi_via_feature(manifest_dir: &Path, bin_name: &str) -> Result<Option<AbiJson>> {
    let source_path = resolve_bin_source_path(manifest_dir, bin_name)?;
    if !source_path.exists() {
        return Ok(None);
    }

    let source_content = fs::read_to_string(&source_path)
        .with_context(|| format!("Failed to read {}", source_path.display()))?;

    if let Some(sol_path) = extract_sol_path_from_source(&source_content) {
        let sol_full_path = manifest_dir.join(sol_path);
        return generate_abi_from_sol(&sol_full_path);
    }

    let target_dir = super::get_target_root().join("abi-gen-target");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest_path = manifest_dir.join("Cargo.toml");

    // The project's .cargo/config.toml targets RISC-V with build-std=core,alloc.
    // The abi-gen binary needs std and must run on the host, so we override both:
    // --target forces the host triple, build-std adds std to the sysroot rebuild.
    let host = env::var("HOST")
        .context("HOST env var not set — generate_abi_via_feature must run from build.rs")?;

    let output = Command::new(&cargo)
        .current_dir(manifest_dir)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC")
        .env("CARGO_TARGET_DIR", &target_dir)
        .env(super::INTERNAL_BUILD_ENV, "1")
        .arg("run")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--target")
        .arg(&host)
        .arg("--config")
        .arg(r#"unstable.build-std=["std","core","alloc"]"#)
        .arg("--features")
        .arg("abi-gen")
        .arg("--bin")
        .arg(bin_name)
        .output()
        .context("Failed to execute abi-gen compile and run")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ABI generation via abi-gen feature failed:\n{stderr}");
    }

    let stdout_str =
        String::from_utf8(output.stdout).context("ABI generation output is not valid UTF-8")?;

    let abi: AbiJson = serde_json::from_str(&stdout_str)
        .context("Failed to parse ABI JSON from abi-gen output")?;

    Ok(Some(abi))
}

fn extract_sol_path_from_source(source: &str) -> Option<String> {
    if let Some(start) = source.find("contract(\"") {
        let after_quote = &source[start + 10..];
        if let Some(end) = after_quote.find('"') {
            let path = &after_quote[..end];
            if path.ends_with(".sol") {
                return Some(path.to_string());
            }
        }
    }
    None
}

fn resolve_bin_source_path(manifest_dir: &Path, bin_name: &str) -> Result<std::path::PathBuf> {
    let cargo_toml_path = manifest_dir.join("Cargo.toml");
    let cargo_toml = std::fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("Failed to read {}", cargo_toml_path.display()))?;
    let doc = cargo_toml
        .parse::<DocumentMut>()
        .context("Failed to parse Cargo.toml")?;

    if let Some(bin_array) = doc.get("bin").and_then(|b| b.as_array_of_tables()) {
        for bin in bin_array {
            if bin.get("name").and_then(|n| n.as_str()) == Some(bin_name) {
                if let Some(path) = bin.get("path").and_then(|p| p.as_str()) {
                    return Ok(manifest_dir.join(path));
                }
                return Ok(manifest_dir.join("src/bin").join(format!("{bin_name}.rs")));
            }
        }
    }

    if doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        == Some(bin_name)
    {
        return Ok(manifest_dir.join("src/main.rs"));
    }

    Ok(manifest_dir.join("src/bin").join(format!("{bin_name}.rs")))
}

fn generate_abi_from_sol(sol_path: &Path) -> Result<Option<AbiJson>> {
    let content = std::fs::read_to_string(sol_path)
        .with_context(|| format!("Failed to read sol file: {}", sol_path.display()))?;

    let mut items = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("function ")
            && let Some(func) = parse_sol_function_line(line)
        {
            items.push(func);
        }
        if line.starts_with("error ")
            && let Some(err) = parse_sol_error_line(line)
        {
            items.push(err);
        }
    }

    if items.is_empty() {
        return Ok(None);
    }

    Ok(Some(AbiJson(items)))
}

fn parse_sol_function_line(line: &str) -> Option<AbiItem> {
    let line = line.strip_prefix("function ")?.trim();

    let paren_start = line.find('(')?;
    let name = line[..paren_start].trim().to_string();

    let paren_end = find_matching_paren(line, paren_start)?;
    let params_str = &line[paren_start + 1..paren_end];
    let inputs = parse_sol_params(params_str);

    let outputs = if let Some(returns_idx) = line.find("returns") {
        let after_returns = &line[returns_idx + 7..];
        if let Some(start) = after_returns.find('(') {
            let end = find_matching_paren(after_returns, start)?;
            parse_sol_params(&after_returns[start + 1..end])
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    let state_mutability = if line.contains(" view ") || line.contains(" view)") {
        "view"
    } else if line.contains(" pure ") || line.contains(" pure)") {
        "pure"
    } else if line.contains(" payable ") || line.contains(" payable)") {
        "payable"
    } else {
        "nonpayable"
    }
    .to_string();

    Some(AbiItem::Function {
        name,
        inputs,
        outputs,
        state_mutability: Some(state_mutability),
    })
}

fn parse_sol_error_line(line: &str) -> Option<AbiItem> {
    let line = line.strip_prefix("error ")?.trim();

    let paren_start = line.find('(')?;
    let name = line[..paren_start].trim().to_string();

    let paren_end = find_matching_paren(line, paren_start)?;
    let params_str = &line[paren_start + 1..paren_end];
    let inputs = parse_sol_params(params_str);

    Some(AbiItem::Error { name, inputs })
}

/// Find the matching closing paren for the opening paren at `start`.
fn find_matching_paren(s: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in s[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split parameters at top-level commas, respecting parenthesis nesting.
fn split_top_level_params(params_str: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut depth = 0;
    let mut current = String::new();

    for ch in params_str.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    params.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        params.push(current.trim().to_string());
    }
    params
}

fn parse_sol_params(params_str: &str) -> Vec<AbiParam> {
    if params_str.trim().is_empty() {
        return vec![];
    }

    split_top_level_params(params_str)
        .iter()
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }
            // Split into tokens, strip Solidity storage keywords (memory, calldata, storage),
            // last token is the name, first token is the type.
            // e.g. "uint256[] calldata ids" → type="uint256[]", name="ids"
            if let Some(last_space) = p.rfind(|c: char| c.is_whitespace()) {
                let name = p[last_space..].trim().to_string();
                let before_name = p[..last_space].trim();
                // Strip storage location keywords between type and name
                let param_type = before_name
                    .split_whitespace()
                    .filter(|t| !matches!(*t, "memory" | "calldata" | "storage"))
                    .collect::<Vec<_>>()
                    .join(" ");
                if !param_type.is_empty() {
                    return Some(AbiParam { name, param_type });
                }
            }
            // No name — type only
            Some(AbiParam {
                name: String::new(),
                param_type: p.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sol_abi_generation() {
        // --- Parse error lines ---

        // Error with params
        assert_eq!(
            parse_sol_error_line(
                "error InsufficientBalance(address account, uint256 required, uint256 available);",
            )
            .unwrap(),
            AbiItem::Error {
                name: "InsufficientBalance".to_string(),
                inputs: vec![
                    AbiParam {
                        name: "account".to_string(),
                        param_type: "address".to_string()
                    },
                    AbiParam {
                        name: "required".to_string(),
                        param_type: "uint256".to_string()
                    },
                    AbiParam {
                        name: "available".to_string(),
                        param_type: "uint256".to_string()
                    },
                ],
            }
        );

        // Error without params
        assert_eq!(
            parse_sol_error_line("error Unauthorized();").unwrap(),
            AbiItem::Error {
                name: "Unauthorized".to_string(),
                inputs: vec![],
            }
        );

        // --- Full .sol file parsing ---

        let sol = r#"
            interface MyToken {
                function transfer(address to, uint256 amount) external;
                error InsufficientBalance(address account, uint256 required);
                error Unauthorized();
            }
        "#;
        let dir = std::env::temp_dir().join("pvm_abi_test_sol_gen");
        let _ = std::fs::create_dir_all(&dir);
        let sol_path = dir.join("MyToken.sol");
        std::fs::write(&sol_path, sol).unwrap();

        let abi = generate_abi_from_sol(&sol_path).unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(abi.0.len(), 3);
        assert!(matches!(&abi.0[0], AbiItem::Function { name, .. } if name == "transfer"));
        assert!(matches!(&abi.0[1], AbiItem::Error { name, .. } if name == "InsufficientBalance"));
        assert!(matches!(&abi.0[2], AbiItem::Error { name, .. } if name == "Unauthorized"));

        // --- Roundtrip: all item types ---

        let abi = AbiJson(vec![
            AbiItem::Constructor {
                inputs: vec![AbiParam {
                    name: "supply".to_string(),
                    param_type: "uint256".to_string(),
                }],
                state_mutability: Some("nonpayable".to_string()),
            },
            AbiItem::Function {
                name: "transfer".to_string(),
                inputs: vec![
                    AbiParam {
                        name: "to".to_string(),
                        param_type: "address".to_string(),
                    },
                    AbiParam {
                        name: "amount".to_string(),
                        param_type: "uint256".to_string(),
                    },
                ],
                outputs: vec![AbiParam {
                    name: "".to_string(),
                    param_type: "bool".to_string(),
                }],
                state_mutability: Some("nonpayable".to_string()),
            },
            AbiItem::Error {
                name: "InsufficientBalance".to_string(),
                inputs: vec![AbiParam {
                    name: "account".to_string(),
                    param_type: "address".to_string(),
                }],
            },
            AbiItem::Error {
                name: "Unauthorized".to_string(),
                inputs: vec![],
            },
        ]);

        let json_str = serde_json::to_string(&abi).unwrap();
        let deserialized: AbiJson = serde_json::from_str(&json_str).unwrap();
        assert_eq!(abi, deserialized);

        let items: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let items = items.as_array().unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0]["type"], "constructor");
        assert_eq!(items[0]["stateMutability"], "nonpayable");
        assert_eq!(items[1]["type"], "function");
        assert_eq!(items[2]["type"], "error");
        assert!(items[2].get("stateMutability").is_none());
        assert_eq!(items[3]["type"], "error");
        assert_eq!(items[3]["inputs"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn sol_abi_tuple_params() {
        // Function with tuple param
        let item = parse_sol_function_line(
            "function swap((address,uint256) order, uint256 minOutput) external",
        )
        .unwrap();
        let AbiItem::Function { inputs, .. } = &item else {
            panic!("Expected Function")
        };
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].param_type, "(address,uint256)");
        assert_eq!(inputs[0].name, "order");
        assert_eq!(inputs[1].param_type, "uint256");

        // Error with tuple param
        let item =
            parse_sol_error_line("error BadSwap((address,uint256) order, uint256 received);")
                .unwrap();
        let AbiItem::Error { inputs, .. } = &item else {
            panic!("Expected Error")
        };
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].param_type, "(address,uint256)");
        assert_eq!(inputs[0].name, "order");

        // Function returning tuple
        let item = parse_sol_function_line(
            "function getOrder() external view returns ((address,uint256))",
        )
        .unwrap();
        let AbiItem::Function { outputs, .. } = &item else {
            panic!("Expected Function")
        };
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].param_type, "(address,uint256)");
    }

    #[test]
    fn sol_abi_strips_storage_keywords() {
        let item = parse_sol_function_line(
            "function process(uint256[] calldata ids, string memory name) external",
        )
        .unwrap();
        let AbiItem::Function { inputs, .. } = &item else {
            panic!("Expected Function")
        };
        assert_eq!(inputs[0].param_type, "uint256[]");
        assert_eq!(inputs[0].name, "ids");
        assert_eq!(inputs[1].param_type, "string");
        assert_eq!(inputs[1].name, "name");

        let item = parse_sol_error_line("error BadData(bytes calldata data);").unwrap();
        let AbiItem::Error { inputs, .. } = &item else {
            panic!("Expected Error")
        };
        assert_eq!(inputs[0].param_type, "bytes");
        assert_eq!(inputs[0].name, "data");
    }
}
