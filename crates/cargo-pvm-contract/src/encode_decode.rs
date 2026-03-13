use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::path::Path;
use tiny_keccak::{Hasher, Keccak};

/// Load an ABI JSON file and return the parsed ABI array.
fn load_abi(abi_path: &Path) -> Result<Vec<Value>> {
    let content = std::fs::read_to_string(abi_path)
        .with_context(|| format!("Failed to read ABI file: {}", abi_path.display()))?;
    let abi: Vec<Value> = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse ABI JSON: {}", abi_path.display()))?;
    Ok(abi)
}

/// Compute the 4-byte Keccak-256 selector for a Solidity function signature.
fn selector_from_signature(sig: &str) -> [u8; 4] {
    let mut hasher = Keccak::v256();
    hasher.update(sig.as_bytes());
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);
    let mut selector = [0u8; 4];
    selector.copy_from_slice(&hash[..4]);
    selector
}

/// Build the canonical Solidity function signature from ABI metadata.
/// e.g. "transfer(address,uint256)"
fn build_function_signature(func: &Value) -> Result<String> {
    let name = func["name"]
        .as_str()
        .ok_or_else(|| anyhow!("ABI entry missing 'name'"))?;
    let inputs = func["inputs"]
        .as_array()
        .ok_or_else(|| anyhow!("ABI entry missing 'inputs'"))?;
    let param_types: Vec<&str> = inputs
        .iter()
        .map(|input| {
            input["type"]
                .as_str()
                .ok_or_else(|| anyhow!("Input missing 'type'"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("{}({})", name, param_types.join(",")))
}

/// Find a function entry in the ABI by name.
fn find_function<'a>(abi: &'a [Value], function_name: &str) -> Result<&'a Value> {
    abi.iter()
        .find(|entry| {
            entry["type"].as_str() == Some("function")
                && entry["name"].as_str() == Some(function_name)
        })
        .ok_or_else(|| anyhow!("Function '{}' not found in ABI", function_name))
}

/// Find a constructor entry in the ABI.
fn find_constructor(abi: &[Value]) -> Result<&Value> {
    abi.iter()
        .find(|entry| entry["type"].as_str() == Some("constructor"))
        .ok_or_else(|| anyhow!("Constructor not found in ABI"))
}

/// Encode a u256 value (from decimal string) into 32-byte big-endian.
fn encode_uint256(value: &str) -> Result<Vec<u8>> {
    // Handle hex values
    if value.starts_with("0x") || value.starts_with("0X") {
        let hex_str = &value[2..];
        let bytes = hex::decode(hex_str)
            .with_context(|| format!("Invalid hex value: {value}"))?;
        let mut padded = vec![0u8; 32];
        if bytes.len() > 32 {
            anyhow::bail!("Hex value too large for uint256: {value}");
        }
        padded[32 - bytes.len()..].copy_from_slice(&bytes);
        return Ok(padded);
    }

    // Parse decimal
    let n: u128 = value
        .parse()
        .with_context(|| format!("Failed to parse '{value}' as uint"))?;
    let mut buf = vec![0u8; 32];
    buf[16..].copy_from_slice(&n.to_be_bytes());
    Ok(buf)
}

/// Encode a bool value into 32 bytes.
fn encode_bool(value: &str) -> Result<Vec<u8>> {
    let b = match value {
        "true" | "1" => true,
        "false" | "0" => false,
        _ => anyhow::bail!("Invalid bool value: {value}"),
    };
    let mut buf = vec![0u8; 32];
    if b {
        buf[31] = 1;
    }
    Ok(buf)
}

/// Encode an address (H160) into 32 bytes (left-padded).
fn encode_address(value: &str) -> Result<Vec<u8>> {
    let hex_str = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(hex_str)
        .with_context(|| format!("Invalid address hex: {value}"))?;
    if bytes.len() != 20 {
        anyhow::bail!("Address must be 20 bytes, got {}", bytes.len());
    }
    let mut buf = vec![0u8; 32];
    buf[12..].copy_from_slice(&bytes);
    Ok(buf)
}

/// Encode bytes (as hex string) into ABI dynamic encoding.
fn encode_bytes(value: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let hex_str = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(hex_str)
        .with_context(|| format!("Invalid bytes hex: {value}"))?;
    let len = bytes.len();
    let head = vec![0u8; 32]; // offset placeholder
    let mut tail = vec![0u8; 32]; // length
    tail[24..].copy_from_slice(&(len as u64).to_be_bytes());
    tail.extend_from_slice(&bytes);
    // Pad to 32-byte boundary
    let padding = (32 - (len % 32)) % 32;
    tail.extend(vec![0u8; padding]);
    Ok((head, tail))
}

/// Encode a single ABI argument based on its Solidity type.
fn encode_param(sol_type: &str, value: &str) -> Result<EncodedParam> {
    if sol_type == "address" {
        Ok(EncodedParam::Static(encode_address(value)?))
    } else if sol_type == "bool" {
        Ok(EncodedParam::Static(encode_bool(value)?))
    } else if sol_type.starts_with("uint") || sol_type.starts_with("int") {
        Ok(EncodedParam::Static(encode_uint256(value)?))
    } else if sol_type == "bytes" || sol_type == "string" {
        let (head, tail) = encode_bytes(value)?;
        Ok(EncodedParam::Dynamic { _head: head, tail })
    } else if sol_type.starts_with("bytes") {
        // bytesN (fixed-size)
        let hex_str = value.strip_prefix("0x").unwrap_or(value);
        let bytes = hex::decode(hex_str)?;
        let mut buf = vec![0u8; 32];
        let len = bytes.len().min(32);
        buf[..len].copy_from_slice(&bytes[..len]);
        Ok(EncodedParam::Static(buf))
    } else {
        anyhow::bail!("Unsupported ABI type: {sol_type}");
    }
}

enum EncodedParam {
    Static(Vec<u8>),
    Dynamic { _head: Vec<u8>, tail: Vec<u8> },
}

/// Encode a function call with arguments into ABI-encoded calldata.
pub fn encode_call(abi_path: &Path, function_name: &str, args: &[String]) -> Result<Vec<u8>> {
    let abi = load_abi(abi_path)?;
    let func = find_function(&abi, function_name)?;
    let sig = build_function_signature(func)?;
    let selector = selector_from_signature(&sig);

    let inputs = func["inputs"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing inputs"))?;

    if inputs.len() != args.len() {
        anyhow::bail!(
            "Function '{}' expects {} arguments, got {}",
            function_name,
            inputs.len(),
            args.len()
        );
    }

    let encoded_params = encode_params(inputs, args)?;

    let mut calldata = selector.to_vec();
    calldata.extend(encoded_params);
    Ok(calldata)
}

/// Encode constructor arguments into ABI-encoded calldata (no selector).
pub fn encode_constructor(abi_path: &Path, args: &[String]) -> Result<Vec<u8>> {
    let abi = load_abi(abi_path)?;
    let constructor = find_constructor(&abi)?;

    let inputs = constructor["inputs"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing constructor inputs"))?;

    if inputs.len() != args.len() {
        anyhow::bail!(
            "Constructor expects {} arguments, got {}",
            inputs.len(),
            args.len()
        );
    }

    encode_params(inputs, args)
}

fn encode_params(inputs: &[Value], args: &[String]) -> Result<Vec<u8>> {
    let mut encoded: Vec<EncodedParam> = Vec::new();
    for (input, arg) in inputs.iter().zip(args.iter()) {
        let sol_type = input["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Input missing 'type'"))?;
        encoded.push(encode_param(sol_type, arg)?);
    }

    // Calculate head size (all params get 32 bytes in the head)
    let head_size = encoded.len() * 32;
    let mut head = Vec::new();
    let mut tail = Vec::new();

    for param in &encoded {
        match param {
            EncodedParam::Static(data) => {
                head.extend_from_slice(data);
            }
            EncodedParam::Dynamic { tail: t, .. } => {
                // Write offset to tail
                let offset = head_size + tail.len();
                let mut offset_bytes = vec![0u8; 32];
                offset_bytes[24..].copy_from_slice(&(offset as u64).to_be_bytes());
                head.extend_from_slice(&offset_bytes);
                tail.extend_from_slice(t);
            }
        }
    }

    let mut result = head;
    result.extend(tail);
    Ok(result)
}

/// Decode calldata using an ABI JSON file.
/// Returns (function_name, Vec<(param_name, param_type, decoded_value)>).
pub fn decode_call(
    abi_path: &Path,
    calldata_hex: &str,
) -> Result<(String, Vec<(String, String, String)>)> {
    let hex_str = calldata_hex.strip_prefix("0x").unwrap_or(calldata_hex);
    let calldata = hex::decode(hex_str)
        .with_context(|| format!("Invalid hex calldata: {calldata_hex}"))?;

    if calldata.len() < 4 {
        anyhow::bail!("Calldata too short (less than 4 bytes for selector)");
    }

    let selector = &calldata[..4];
    let data = &calldata[4..];

    let abi = load_abi(abi_path)?;

    // Find the function that matches this selector
    let func = abi
        .iter()
        .filter(|entry| entry["type"].as_str() == Some("function"))
        .find(|entry| {
            build_function_signature(entry)
                .map(|sig| selector_from_signature(&sig) == selector)
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            anyhow!(
                "No function found matching selector 0x{}",
                hex::encode(selector)
            )
        })?;

    let function_name = func["name"]
        .as_str()
        .ok_or_else(|| anyhow!("Function missing name"))?
        .to_string();

    let inputs = func["inputs"]
        .as_array()
        .ok_or_else(|| anyhow!("Function missing inputs"))?;

    let decoded = decode_params(inputs, data)?;
    Ok((function_name, decoded))
}

/// Decode constructor calldata using an ABI JSON file.
pub fn decode_constructor(
    abi_path: &Path,
    calldata_hex: &str,
) -> Result<Vec<(String, String, String)>> {
    let hex_str = calldata_hex.strip_prefix("0x").unwrap_or(calldata_hex);
    let data =
        hex::decode(hex_str).with_context(|| "Invalid hex for constructor data")?;

    let abi = load_abi(abi_path)?;
    let constructor = find_constructor(&abi)?;

    let inputs = constructor["inputs"]
        .as_array()
        .ok_or_else(|| anyhow!("Constructor missing inputs"))?;

    decode_params(inputs, &data)
}

fn decode_params(
    inputs: &[Value],
    data: &[u8],
) -> Result<Vec<(String, String, String)>> {
    let mut results = Vec::new();

    for (i, input) in inputs.iter().enumerate() {
        let name = input["name"]
            .as_str()
            .unwrap_or(&format!("param{i}"))
            .to_string();
        let sol_type = input["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Input missing 'type'"))?
            .to_string();

        let offset = i * 32;
        if offset + 32 > data.len() {
            anyhow::bail!("Calldata too short to decode parameter '{}'", name);
        }

        let word = &data[offset..offset + 32];
        let value = decode_word(&sol_type, word, data)?;
        results.push((name, sol_type, value));
    }

    Ok(results)
}

fn decode_word(sol_type: &str, word: &[u8], _full_data: &[u8]) -> Result<String> {
    if sol_type == "address" {
        Ok(format!("0x{}", hex::encode(&word[12..])))
    } else if sol_type == "bool" {
        Ok(if word[31] != 0 { "true" } else { "false" }.to_string())
    } else if sol_type.starts_with("uint") {
        // Decode as u128 (sufficient for most cases)
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&word[16..]);
        let n = u128::from_be_bytes(bytes);
        Ok(n.to_string())
    } else if sol_type.starts_with("int") {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&word[16..]);
        let n = i128::from_be_bytes(bytes);
        Ok(n.to_string())
    } else if sol_type.starts_with("bytes") && sol_type != "bytes" {
        // Fixed-size bytesN
        let size: usize = sol_type[5..].parse().unwrap_or(32);
        Ok(format!("0x{}", hex::encode(&word[..size])))
    } else {
        // For dynamic types, show the offset
        let mut offset_bytes = [0u8; 8];
        offset_bytes.copy_from_slice(&word[24..]);
        let offset = u64::from_be_bytes(offset_bytes) as usize;
        Ok(format!("(dynamic@offset:{offset})"))
    }
}
