use quote::quote;

#[derive(Debug, Clone, PartialEq)]
pub enum SolType {
    Address,
    Bool,
    Uint(usize),
    Int(usize),
    Bytes(usize),
    DynBytes,
    String,
    Array(Box<SolType>),
    FixedArray(Box<SolType>, usize),
    Tuple(Vec<SolType>),
    Custom(String),
}

impl SolType {
    pub fn canonical_name(&self) -> String {
        match self {
            SolType::Address => "address".to_string(),
            SolType::Bool => "bool".to_string(),
            SolType::Uint(bits) => format!("uint{bits}"),
            SolType::Int(bits) => format!("int{bits}"),
            SolType::Bytes(size) => format!("bytes{size}"),
            SolType::DynBytes => "bytes".to_string(),
            SolType::String => "string".to_string(),
            SolType::Array(inner) => format!("{}[]", inner.canonical_name()),
            SolType::FixedArray(inner, size) => format!("{}[{}]", inner.canonical_name(), size),
            SolType::Tuple(types) => {
                let inner: Vec<_> = types.iter().map(|t| t.canonical_name()).collect();
                format!("({})", inner.join(","))
            }
            SolType::Custom(name) => name.clone(),
        }
    }

    pub fn is_dynamic(&self) -> bool {
        match self {
            SolType::DynBytes | SolType::String | SolType::Array(_) => true,
            SolType::Tuple(types) => types.iter().any(|t| t.is_dynamic()),
            SolType::FixedArray(inner, _) => inner.is_dynamic(),
            SolType::Custom(_) => false,
            _ => false,
        }
    }

    pub fn head_size(&self) -> usize {
        match self {
            SolType::FixedArray(inner, size) if !inner.is_dynamic() => inner.head_size() * size,
            SolType::Tuple(types) if !self.is_dynamic() => {
                types.iter().map(|t| t.head_size()).sum()
            }
            SolType::Custom(_) => 0,
            _ => 32,
        }
    }

    pub fn has_custom_types(&self) -> bool {
        match self {
            SolType::Custom(_) => true,
            SolType::Array(inner) | SolType::FixedArray(inner, _) => inner.has_custom_types(),
            SolType::Tuple(types) => types.iter().any(|t| t.has_custom_types()),
            _ => false,
        }
    }

    pub fn from_rust_type(ty: &syn::Type) -> Option<Self> {
        // Handle Vec<T> and alloc::vec::Vec<T> patterns
        if let syn::Type::Path(type_path) = ty {
            let last_segment = type_path.path.segments.last()?;
            if last_segment.ident == "Vec"
                && let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments
                && let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first()
            {
                return Self::from_rust_type(inner_ty).map(|inner| SolType::Array(Box::new(inner)));
            }
        }

        if let syn::Type::Array(array) = ty
            && let syn::Expr::Lit(expr_lit) = &array.len
            && let syn::Lit::Int(lit_int) = &expr_lit.lit
            && let Ok(size) = lit_int.base10_parse::<usize>()
        {
            let inner = Self::from_rust_type(&array.elem)?;
            return Some(SolType::FixedArray(Box::new(inner), size));
        }

        if let syn::Type::Tuple(tuple) = ty {
            let elems = tuple
                .elems
                .iter()
                .map(Self::from_rust_type)
                .collect::<Option<Vec<_>>>()?;
            return Some(SolType::Tuple(elems));
        }

        let type_str = quote!(#ty).to_string().replace(' ', "");

        match type_str.as_str() {
            "Address"
            | "pvm_contract_types::Address"
            | "::pvm_contract_types::Address"
            | "pvm_contract::Address"
            | "::pvm_contract::Address" => Some(SolType::Address),
            "[u8;20]" => Some(SolType::Address),
            "U256" | "ruint::aliases::U256" => Some(SolType::Uint(256)),
            "u256" => Some(SolType::Uint(256)),
            "u128" => Some(SolType::Uint(128)),
            "u64" => Some(SolType::Uint(64)),
            "u32" => Some(SolType::Uint(32)),
            "u16" => Some(SolType::Uint(16)),
            "u8" => Some(SolType::Uint(8)),
            "i128" => Some(SolType::Int(128)),
            "i64" => Some(SolType::Int(64)),
            "i32" => Some(SolType::Int(32)),
            "i16" => Some(SolType::Int(16)),
            "i8" => Some(SolType::Int(8)),
            "bool" => Some(SolType::Bool),
            "[u8;32]" => Some(SolType::Bytes(32)),
            "String" | "alloc::string::String" => Some(SolType::String),
            _ => Some(SolType::Custom(type_str)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SolType;

    #[test]
    fn maps_address_newtype_to_solidity_address() {
        let ty: syn::Type = syn::parse_str("Address").unwrap();
        let sol = SolType::from_rust_type(&ty).unwrap();
        assert_eq!(sol.canonical_name(), "address");

        let ty: syn::Type = syn::parse_str("pvm_contract_types::Address").unwrap();
        let sol = SolType::from_rust_type(&ty).unwrap();
        assert_eq!(sol.canonical_name(), "address");
    }

    #[test]
    fn maps_fixed_arrays_and_tuples() {
        let ty: syn::Type = syn::parse_str("[u64; 4]").unwrap();
        let sol = SolType::from_rust_type(&ty).unwrap();
        assert_eq!(sol.canonical_name(), "uint64[4]");

        let ty: syn::Type = syn::parse_str("(Address, u256)").unwrap();
        let sol = SolType::from_rust_type(&ty).unwrap();
        assert_eq!(sol.canonical_name(), "(address,uint256)");
    }

    #[test]
    fn maps_custom_paths_to_custom_type() {
        let ty: syn::Type = syn::parse_str("my_crate::Foo").unwrap();
        let sol = SolType::from_rust_type(&ty).unwrap();
        assert_eq!(sol.canonical_name(), "my_crate::Foo");
    }

    // --- Type alias brittleness tests ---
    // These tests assert CORRECT behavior. They currently FAIL, documenting the
    // string-matching bug: `type Count = u64` appears as "Count" to the proc macro,
    // falls to SolType::Custom("Count"), and downstream code gets wrong selectors,
    // buffer sizes, and ABI names.

    #[test]
    fn type_alias_canonical_name_should_match_underlying_type() {
        // `type Count = u64` — macro sees "Count", not "u64"
        let ty: syn::Type = syn::parse_str("Count").unwrap();
        let sol = SolType::from_rust_type(&ty).unwrap();
        // BUG: returns "Count" instead of "uint64"
        assert_eq!(
            sol.canonical_name(),
            "uint64",
            "Type alias canonical name should resolve to underlying Solidity type, \
             not the Rust alias name. Got SolType::{:?}",
            sol
        );
    }

    #[test]
    fn custom_type_head_size_must_not_be_zero() {
        // Any 32-byte-word type alias (u64, U256, Address, etc.) should report
        // head_size=32. Custom returns 0, causing:
        // - buffer underallocation in #[derive(SolType)] structs
        // - overlapping field offsets in encode/decode
        // - wrong min_input_size in dispatch
        let sol = SolType::Custom("Count".to_string());
        assert_ne!(
            sol.head_size(),
            0,
            "Custom type head_size=0 causes buffer underallocation and offset overlap"
        );
    }

    #[test]
    fn selector_for_alias_must_match_concrete_type() {
        use crate::signature::compute_selector;

        // A method `setCount(Count)` where `type Count = u64` should produce
        // the same selector as `setCount(uint64)`. The ABI advertises uint64,
        // so callers compute keccak("setCount(uint64)"). If the dispatch matches
        // on keccak("setCount(Count)"), no call ever reaches the method.
        let alias_inputs = vec![SolType::Custom("Count".to_string())];
        let concrete_inputs = vec![SolType::Uint(64)];

        let alias_sig = format!(
            "setCount({})",
            alias_inputs
                .iter()
                .map(|t| t.canonical_name())
                .collect::<Vec<_>>()
                .join(",")
        );
        let concrete_sig = format!(
            "setCount({})",
            concrete_inputs
                .iter()
                .map(|t| t.canonical_name())
                .collect::<Vec<_>>()
                .join(",")
        );

        assert_eq!(
            compute_selector(&alias_sig),
            compute_selector(&concrete_sig),
            "Selector mismatch: alias sig '{}' vs concrete sig '{}' — contract unreachable",
            alias_sig,
            concrete_sig,
        );
    }
}
