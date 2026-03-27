use anyhow::{Context, Result, anyhow};
use ruint::aliases::U256;
use serde_json::Value;
use std::path::Path;
use tiny_keccak::{Hasher, Keccak};

/// A decoded ABI parameter.
pub struct DecodedParam {
    pub name: String,
    pub sol_type: String,
    pub value: String,
}

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

/// Encode a uint256 value (from decimal or hex string) into 32-byte big-endian.
fn encode_uint256(value: &str) -> Result<Vec<u8>> {
    let n: U256 = value
        .parse()
        .map_err(|_| anyhow!("Failed to parse '{value}' as uint256"))?;
    Ok(n.to_be_bytes::<32>().to_vec())
}

/// Decode a 32-byte big-endian word as an unsigned decimal string.
fn u256_be_to_decimal(bytes: &[u8]) -> String {
    U256::from_be_slice(bytes).to_string()
}

/// Decode a 32-byte big-endian word as a signed (two's complement) decimal string.
fn i256_be_to_decimal(bytes: &[u8]) -> String {
    let n = U256::from_be_slice(bytes);
    let sign_bit = U256::from(1) << 255;
    if n < sign_bit {
        n.to_string()
    } else {
        // Negative: compute -(2^256 - n)
        let abs = (!n).wrapping_add(U256::from(1));
        format!("-{abs}")
    }
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
    let bytes = hex::decode(hex_str).with_context(|| format!("Invalid address hex: {value}"))?;
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
    let bytes = hex::decode(hex_str).with_context(|| format!("Invalid bytes hex: {value}"))?;
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

/// Encode a Solidity `string` value from raw UTF-8 into ABI dynamic encoding.
fn encode_string(value: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let bytes = value.as_bytes();
    let len = bytes.len();
    let head = vec![0u8; 32]; // offset placeholder
    let mut tail = vec![0u8; 32]; // length
    tail[24..].copy_from_slice(&(len as u64).to_be_bytes());
    tail.extend_from_slice(bytes);
    // Pad to 32-byte boundary
    let padding = (32 - (len % 32)) % 32;
    tail.extend(vec![0u8; padding]);
    Ok((head, tail))
}

/// Encode a dynamic array value (e.g. `[1,2,3]`) into ABI dynamic encoding.
fn encode_array(inner_type: &str, value: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| anyhow!("Array value must be wrapped in brackets: {value}"))?;

    let elements: Vec<&str> = if inner.trim().is_empty() {
        vec![]
    } else {
        inner.split(',').map(|s| s.trim()).collect()
    };

    let head = vec![0u8; 32]; // offset placeholder

    // length word
    let mut tail = vec![0u8; 32];
    tail[24..].copy_from_slice(&(elements.len() as u64).to_be_bytes());

    // Encode each element (only static inner types supported for now)
    for elem in &elements {
        let encoded = encode_param(inner_type, elem)?;
        match encoded {
            EncodedParam::Static(data) => tail.extend_from_slice(&data),
            EncodedParam::Dynamic { .. } => {
                anyhow::bail!("Arrays of dynamic types not yet supported");
            }
        }
    }

    Ok((head, tail))
}

/// Encode a single ABI argument based on its Solidity type.
fn encode_param(sol_type: &str, value: &str) -> Result<EncodedParam> {
    // Check dynamic array first (e.g. "uint256[]") before prefix matches
    if sol_type.ends_with("[]") {
        let inner_type = &sol_type[..sol_type.len() - 2];
        let (head, tail) = encode_array(inner_type, value)?;
        Ok(EncodedParam::Dynamic { _head: head, tail })
    } else if sol_type == "address" {
        Ok(EncodedParam::Static(encode_address(value)?))
    } else if sol_type == "bool" {
        Ok(EncodedParam::Static(encode_bool(value)?))
    } else if sol_type.starts_with("uint") || sol_type.starts_with("int") {
        Ok(EncodedParam::Static(encode_uint256(value)?))
    } else if sol_type == "string" {
        let (head, tail) = encode_string(value)?;
        Ok(EncodedParam::Dynamic { _head: head, tail })
    } else if sol_type == "bytes" {
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
pub fn decode_call(abi_path: &Path, calldata_hex: &str) -> Result<(String, Vec<DecodedParam>)> {
    let hex_str = calldata_hex.strip_prefix("0x").unwrap_or(calldata_hex);
    let calldata =
        hex::decode(hex_str).with_context(|| format!("Invalid hex calldata: {calldata_hex}"))?;

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
pub fn decode_constructor(abi_path: &Path, calldata_hex: &str) -> Result<Vec<DecodedParam>> {
    let hex_str = calldata_hex.strip_prefix("0x").unwrap_or(calldata_hex);
    let data = hex::decode(hex_str).with_context(|| "Invalid hex for constructor data")?;

    let abi = load_abi(abi_path)?;
    let constructor = find_constructor(&abi)?;

    let inputs = constructor["inputs"]
        .as_array()
        .ok_or_else(|| anyhow!("Constructor missing inputs"))?;

    decode_params(inputs, &data)
}

fn decode_params(inputs: &[Value], data: &[u8]) -> Result<Vec<DecodedParam>> {
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
        results.push(DecodedParam {
            name,
            sol_type,
            value,
        });
    }

    Ok(results)
}

fn decode_word(sol_type: &str, word: &[u8], full_data: &[u8]) -> Result<String> {
    // Check dynamic array first (e.g. "uint256[]") before prefix matches
    if sol_type.ends_with("[]") {
        let inner_type = &sol_type[..sol_type.len() - 2];
        let mut offset_bytes = [0u8; 8];
        offset_bytes.copy_from_slice(&word[24..]);
        let offset = u64::from_be_bytes(offset_bytes) as usize;
        if offset + 32 > full_data.len() {
            anyhow::bail!("Array offset out of bounds for '{sol_type}'");
        }
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&full_data[offset + 24..offset + 32]);
        let count = u64::from_be_bytes(len_bytes) as usize;
        let data_start = offset + 32;
        let mut items = Vec::new();
        for i in 0..count {
            let elem_offset = data_start + i * 32;
            if elem_offset + 32 > full_data.len() {
                anyhow::bail!("Array element {i} out of bounds for '{sol_type}'");
            }
            let elem_word = &full_data[elem_offset..elem_offset + 32];
            items.push(decode_word(inner_type, elem_word, full_data)?);
        }
        Ok(format!("[{}]", items.join(",")))
    } else if sol_type == "address" {
        Ok(format!("0x{}", hex::encode(&word[12..])))
    } else if sol_type == "bool" {
        Ok(if word[31] != 0 { "true" } else { "false" }.to_string())
    } else if sol_type.starts_with("uint") {
        Ok(u256_be_to_decimal(word))
    } else if sol_type.starts_with("int") {
        Ok(i256_be_to_decimal(word))
    } else if sol_type.starts_with("bytes") && sol_type != "bytes" {
        // Fixed-size bytesN
        let size: usize = sol_type[5..].parse().unwrap_or(32);
        Ok(format!("0x{}", hex::encode(&word[..size])))
    } else if sol_type == "string" || sol_type == "bytes" {
        // Dynamic type: word contains the offset into full_data
        let mut offset_bytes = [0u8; 8];
        offset_bytes.copy_from_slice(&word[24..]);
        let offset = u64::from_be_bytes(offset_bytes) as usize;
        if offset + 32 > full_data.len() {
            anyhow::bail!("Dynamic data offset out of bounds for '{sol_type}'");
        }
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&full_data[offset + 24..offset + 32]);
        let len = u64::from_be_bytes(len_bytes) as usize;
        let data_start = offset + 32;
        if data_start + len > full_data.len() {
            anyhow::bail!("Dynamic data length out of bounds for '{sol_type}'");
        }
        let raw = &full_data[data_start..data_start + len];
        if sol_type == "string" {
            Ok(String::from_utf8_lossy(raw).to_string())
        } else {
            Ok(format!("0x{}", hex::encode(raw)))
        }
    } else {
        anyhow::bail!("Unsupported ABI type for decoding: {sol_type}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_abi_file(abi_json: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(abi_json.as_bytes()).unwrap();
        f
    }

    // -- selector tests --

    #[test]
    fn selector_transfer() {
        // keccak256("transfer(address,uint256)") = 0xa9059cbb...
        let sel = selector_from_signature("transfer(address,uint256)");
        assert_eq!(sel, [0xa9, 0x05, 0x9c, 0xbb]);
    }

    // -- build_function_signature tests --

    #[test]
    fn build_sig_from_abi_entry() {
        let entry: Value = serde_json::from_str(
            r#"{"name":"transfer","inputs":[{"type":"address"},{"type":"uint256"}]}"#,
        )
        .unwrap();
        assert_eq!(
            build_function_signature(&entry).unwrap(),
            "transfer(address,uint256)"
        );
    }

    #[test]
    fn build_sig_no_params() {
        let entry: Value = serde_json::from_str(r#"{"name":"totalSupply","inputs":[]}"#).unwrap();
        assert_eq!(build_function_signature(&entry).unwrap(), "totalSupply()");
    }

    // -- encode primitive tests --

    #[test]
    fn encode_uint256_decimal() {
        let encoded = encode_uint256("42").unwrap();
        assert_eq!(encoded.len(), 32);
        assert_eq!(encoded[31], 42);
        assert!(encoded[..31].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_uint256_hex() {
        let encoded = encode_uint256("0xff").unwrap();
        assert_eq!(encoded.len(), 32);
        assert_eq!(encoded[31], 0xff);
        assert!(encoded[..31].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_uint256_large() {
        let encoded = encode_uint256("1000000000000000000").unwrap(); // 1e18
        assert_eq!(encoded.len(), 32);
        let mut expected = [0u8; 16];
        expected.copy_from_slice(&1_000_000_000_000_000_000u128.to_be_bytes());
        assert_eq!(&encoded[16..], &expected);
    }

    #[test]
    fn encode_uint256_larger_than_u128() {
        // 2^128 = 340282366920938463463374607431768211456
        let encoded = encode_uint256("340282366920938463463374607431768211456").unwrap();
        assert_eq!(encoded.len(), 32);
        // 2^128 in big-endian: byte 15 = 0x01, rest zero
        assert_eq!(encoded[15], 1);
        assert!(encoded[..15].iter().all(|&b| b == 0));
        assert!(encoded[16..].iter().all(|&b| b == 0));
    }

    #[test]
    fn decode_uint256_larger_than_u128() {
        let mut word = [0u8; 32];
        word[15] = 1; // 2^128
        assert_eq!(
            u256_be_to_decimal(&word),
            "340282366920938463463374607431768211456"
        );
    }

    #[test]
    fn encode_bool_true() {
        let encoded = encode_bool("true").unwrap();
        assert_eq!(encoded[31], 1);
        assert!(encoded[..31].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_bool_false() {
        let encoded = encode_bool("false").unwrap();
        assert!(encoded.iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_bool_invalid() {
        assert!(encode_bool("maybe").is_err());
    }

    #[test]
    fn encode_address_with_prefix() {
        let addr = "0x0000000000000000000000000000000000000001";
        let encoded = encode_address(addr).unwrap();
        assert_eq!(encoded.len(), 32);
        assert_eq!(encoded[31], 1);
        assert!(encoded[..31].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_address_without_prefix() {
        let addr = "0000000000000000000000000000000000000001";
        let encoded = encode_address(addr).unwrap();
        assert_eq!(encoded[31], 1);
    }

    #[test]
    fn encode_address_wrong_length() {
        assert!(encode_address("0x0011").is_err());
    }

    // -- encode_bytes tests --

    #[test]
    fn encode_bytes_pads_to_32() {
        let (_head, tail) = encode_bytes("0xdeadbeef").unwrap();
        // tail = 32-byte length + data padded to 32 bytes
        assert_eq!(tail.len(), 64); // 32 (length) + 32 (4 bytes padded)
        // length word: 4
        assert_eq!(tail[31], 4);
        // data starts at offset 32
        assert_eq!(&tail[32..36], &[0xde, 0xad, 0xbe, 0xef]);
        // rest is zero-padded
        assert!(tail[36..].iter().all(|&b| b == 0));
    }

    // -- encode_string tests --

    #[test]
    fn encode_string_utf8() {
        let (_head, tail) = encode_string("hello").unwrap();
        // tail = 32-byte length + data padded to 32 bytes
        assert_eq!(tail.len(), 64); // 32 (length) + 32 (5 bytes padded)
        // length word: 5
        assert_eq!(tail[31], 5);
        // data is raw UTF-8, not hex-decoded
        assert_eq!(&tail[32..37], b"hello");
        // rest is zero-padded
        assert!(tail[37..].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_param_string_uses_utf8_not_hex() {
        // "string" type should encode the raw UTF-8 value, not hex-decode it
        let result = encode_param("string", "hello world").unwrap();
        match result {
            EncodedParam::Dynamic { tail, .. } => {
                // length = 11 bytes ("hello world")
                assert_eq!(tail[31], 11);
                assert_eq!(&tail[32..43], b"hello world");
            }
            EncodedParam::Static(_) => panic!("Expected dynamic encoding for string"),
        }
    }

    #[test]
    fn encode_param_bytes_still_uses_hex() {
        // "bytes" type should still hex-decode the input
        let result = encode_param("bytes", "0xdeadbeef").unwrap();
        match result {
            EncodedParam::Dynamic { tail, .. } => {
                assert_eq!(tail[31], 4); // length = 4 decoded bytes
                assert_eq!(&tail[32..36], &[0xde, 0xad, 0xbe, 0xef]);
            }
            EncodedParam::Static(_) => panic!("Expected dynamic encoding for bytes"),
        }
    }

    // -- encode_array tests --

    #[test]
    fn encode_array_uint256() {
        let (_head, tail) = encode_array("uint256", "[1,2,3]").unwrap();
        // tail = 32-byte length(3) + 3 x 32-byte elements
        assert_eq!(tail.len(), 32 + 3 * 32);
        assert_eq!(tail[31], 3); // length = 3
        assert_eq!(tail[32 + 31], 1); // first element
        assert_eq!(tail[64 + 31], 2); // second element
        assert_eq!(tail[96 + 31], 3); // third element
    }

    #[test]
    fn encode_array_empty() {
        let (_head, tail) = encode_array("uint256", "[]").unwrap();
        assert_eq!(tail.len(), 32); // just the length word
        assert_eq!(tail[31], 0); // length = 0
    }

    // -- encode_param tests --

    #[test]
    fn encode_param_bytesn() {
        let result = encode_param("bytes4", "0xdeadbeef").unwrap();
        match result {
            EncodedParam::Static(data) => {
                assert_eq!(&data[..4], &[0xde, 0xad, 0xbe, 0xef]);
                assert!(data[4..].iter().all(|&b| b == 0));
            }
            EncodedParam::Dynamic { .. } => panic!("Expected static encoding for bytes4"),
        }
    }

    #[test]
    fn encode_param_unsupported() {
        assert!(encode_param("tuple", "anything").is_err());
    }

    // -- full encode/decode roundtrip via ABI files --

    const ERC20_ABI: &str = r#"[
        {
            "type": "function",
            "name": "transfer",
            "inputs": [
                {"name": "to", "type": "address"},
                {"name": "amount", "type": "uint256"}
            ],
            "outputs": [{"name": "", "type": "bool"}]
        },
        {
            "type": "function",
            "name": "balanceOf",
            "inputs": [
                {"name": "account", "type": "address"}
            ],
            "outputs": [{"name": "", "type": "uint256"}]
        },
        {
            "type": "constructor",
            "inputs": [
                {"name": "initialSupply", "type": "uint256"}
            ]
        }
    ]"#;

    #[test]
    fn encode_call_transfer() {
        let f = write_abi_file(ERC20_ABI);
        let args = vec![
            "0x0000000000000000000000000000000000000001".to_string(),
            "100".to_string(),
        ];
        let calldata = encode_call(f.path(), "transfer", &args).unwrap();
        // First 4 bytes: selector for transfer(address,uint256)
        assert_eq!(&calldata[..4], &[0xa9, 0x05, 0x9c, 0xbb]);
        // Next 32 bytes: address padded
        assert_eq!(calldata[4 + 31], 1);
        // Next 32 bytes: uint256(100)
        assert_eq!(calldata[4 + 32 + 31], 100);
        assert_eq!(calldata.len(), 4 + 64);
    }

    #[test]
    fn encode_call_wrong_arg_count() {
        let f = write_abi_file(ERC20_ABI);
        let args = vec!["0x0000000000000000000000000000000000000001".to_string()];
        assert!(encode_call(f.path(), "transfer", &args).is_err());
    }

    #[test]
    fn encode_call_unknown_function() {
        let f = write_abi_file(ERC20_ABI);
        assert!(encode_call(f.path(), "nonexistent", &[]).is_err());
    }

    #[test]
    fn encode_constructor_uint() {
        let f = write_abi_file(ERC20_ABI);
        let args = vec!["1000000".to_string()];
        let calldata = encode_constructor(f.path(), &args).unwrap();
        // No selector for constructors
        assert_eq!(calldata.len(), 32);
        // Decode back the value
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&calldata[16..]);
        assert_eq!(u128::from_be_bytes(bytes), 1_000_000);
    }

    #[test]
    fn decode_call_transfer() {
        let f = write_abi_file(ERC20_ABI);
        let args = vec![
            "0x0000000000000000000000000000000000000001".to_string(),
            "100".to_string(),
        ];
        let calldata = encode_call(f.path(), "transfer", &args).unwrap();
        let hex_data = format!("0x{}", hex::encode(&calldata));

        let (name, params) = decode_call(f.path(), &hex_data).unwrap();
        assert_eq!(name, "transfer");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "to");
        assert_eq!(params[0].sol_type, "address");
        assert_eq!(
            params[0].value,
            "0x0000000000000000000000000000000000000001"
        );
        assert_eq!(params[1].name, "amount");
        assert_eq!(params[1].sol_type, "uint256");
        assert_eq!(params[1].value, "100");
    }

    #[test]
    fn decode_call_balanceof() {
        let f = write_abi_file(ERC20_ABI);
        let args = vec!["0x000000000000000000000000000000000000abcd".to_string()];
        let calldata = encode_call(f.path(), "balanceOf", &args).unwrap();
        let hex_data = format!("0x{}", hex::encode(&calldata));

        let (name, params) = decode_call(f.path(), &hex_data).unwrap();
        assert_eq!(name, "balanceOf");
        assert_eq!(params.len(), 1);
        assert_eq!(
            params[0].value,
            "0x000000000000000000000000000000000000abcd"
        );
    }

    #[test]
    fn decode_constructor_roundtrip() {
        let f = write_abi_file(ERC20_ABI);
        let args = vec!["999".to_string()];
        let calldata = encode_constructor(f.path(), &args).unwrap();
        let hex_data = format!("0x{}", hex::encode(&calldata));

        let params = decode_constructor(f.path(), &hex_data).unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "initialSupply");
        assert_eq!(params[0].sol_type, "uint256");
        assert_eq!(params[0].value, "999");
    }

    #[test]
    fn decode_call_too_short() {
        let f = write_abi_file(ERC20_ABI);
        assert!(decode_call(f.path(), "0xaa").is_err());
    }

    #[test]
    fn decode_call_unknown_selector() {
        let f = write_abi_file(ERC20_ABI);
        // Valid length but bogus selector + 32 bytes of data
        let data = format!("0x{}{}", "deadbeef", "00".repeat(32));
        assert!(decode_call(f.path(), &data).is_err());
    }

    // -- decode_word tests --

    #[test]
    fn decode_word_bool() {
        let mut word = [0u8; 32];
        word[31] = 1;
        assert_eq!(decode_word("bool", &word, &[]).unwrap(), "true");
        word[31] = 0;
        assert_eq!(decode_word("bool", &word, &[]).unwrap(), "false");
    }

    #[test]
    fn decode_word_int256_negative() {
        // -1 in two's complement: all 0xff
        let word = [0xffu8; 32];
        assert_eq!(decode_word("int256", &word, &[]).unwrap(), "-1");
    }

    #[test]
    fn decode_word_bytes4() {
        let mut word = [0u8; 32];
        word[0] = 0xde;
        word[1] = 0xad;
        word[2] = 0xbe;
        word[3] = 0xef;
        assert_eq!(decode_word("bytes4", &word, &[]).unwrap(), "0xdeadbeef");
    }

    // -- string/bytes ABI roundtrip --

    #[test]
    fn encode_decode_call_with_string_param() {
        let abi = r#"[{
            "type": "function",
            "name": "setName",
            "inputs": [
                {"name": "owner", "type": "address"},
                {"name": "name", "type": "string"}
            ],
            "outputs": []
        }]"#;
        let f = write_abi_file(abi);
        let args = vec![
            "0x000000000000000000000000000000000000CAFE".to_string(),
            "hello world".to_string(),
        ];
        let calldata = encode_call(f.path(), "setName", &args).unwrap();
        let hex_data = format!("0x{}", hex::encode(&calldata));

        let (name, params) = decode_call(f.path(), &hex_data).unwrap();
        assert_eq!(name, "setName");
        assert_eq!(params.len(), 2);
        assert_eq!(
            params[0].value,
            "0x000000000000000000000000000000000000cafe"
        );
        assert_eq!(params[0].sol_type, "address");
        assert_eq!(params[1].sol_type, "string");
        assert_eq!(params[1].value, "hello world");
    }

    #[test]
    fn encode_decode_call_with_string_and_uint() {
        let abi = r#"[{
            "type": "function",
            "name": "register",
            "inputs": [
                {"name": "label", "type": "string"},
                {"name": "amount", "type": "uint256"}
            ],
            "outputs": []
        }]"#;
        let f = write_abi_file(abi);
        let args = vec!["my-token".to_string(), "42".to_string()];
        let calldata = encode_call(f.path(), "register", &args).unwrap();
        let hex_data = format!("0x{}", hex::encode(&calldata));

        let (name, params) = decode_call(f.path(), &hex_data).unwrap();
        assert_eq!(name, "register");
        assert_eq!(params[0].value, "my-token");
        assert_eq!(params[1].value, "42");
    }

    // -- array ABI roundtrip --

    #[test]
    fn encode_decode_call_with_array() {
        let abi = r#"[{
            "type": "function",
            "name": "sumArray",
            "inputs": [
                {"name": "arr", "type": "uint256[]"}
            ],
            "outputs": [{"name": "", "type": "uint256"}]
        }]"#;
        let f = write_abi_file(abi);
        let args = vec!["[1,2,3]".to_string()];
        let calldata = encode_call(f.path(), "sumArray", &args).unwrap();
        let hex_data = format!("0x{}", hex::encode(&calldata));

        let (name, params) = decode_call(f.path(), &hex_data).unwrap();
        assert_eq!(name, "sumArray");
        assert_eq!(params[0].sol_type, "uint256[]");
        assert_eq!(params[0].value, "[1,2,3]");
    }

    #[test]
    fn encode_decode_call_with_empty_array() {
        let abi = r#"[{
            "type": "function",
            "name": "sumArray",
            "inputs": [
                {"name": "arr", "type": "uint256[]"}
            ],
            "outputs": [{"name": "", "type": "uint256"}]
        }]"#;
        let f = write_abi_file(abi);
        let args = vec!["[]".to_string()];
        let calldata = encode_call(f.path(), "sumArray", &args).unwrap();
        let hex_data = format!("0x{}", hex::encode(&calldata));

        let (name, params) = decode_call(f.path(), &hex_data).unwrap();
        assert_eq!(name, "sumArray");
        assert_eq!(params[0].value, "[]");
    }

    // -- multi-type ABI roundtrip --

    #[test]
    fn encode_decode_mixed_types() {
        let abi = r#"[{
            "type": "function",
            "name": "doStuff",
            "inputs": [
                {"name": "flag", "type": "bool"},
                {"name": "addr", "type": "address"},
                {"name": "count", "type": "uint128"}
            ],
            "outputs": []
        }]"#;
        let f = write_abi_file(abi);
        let args = vec![
            "true".to_string(),
            "0x000000000000000000000000000000000000CAFE".to_string(),
            "12345".to_string(),
        ];
        let calldata = encode_call(f.path(), "doStuff", &args).unwrap();
        let hex_data = format!("0x{}", hex::encode(&calldata));

        let (name, params) = decode_call(f.path(), &hex_data).unwrap();
        assert_eq!(name, "doStuff");
        assert_eq!(params[0].value, "true");
        assert_eq!(
            params[1].value,
            "0x000000000000000000000000000000000000cafe"
        );
        assert_eq!(params[2].value, "12345");
    }
}
