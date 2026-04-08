use proc_macro2::TokenStream;
use quote::quote;

use super::contract::ParsedContract;
use super::dispatch::MethodInfo;
use crate::signature::SolType;

pub fn generate_abi_gen_main(parsed: &ParsedContract, has_sol_path: bool) -> TokenStream {
    if has_sol_path {
        return quote! {};
    }

    match generate_abi_gen_main_impl(parsed) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    }
}

fn generate_abi_gen_main_impl(parsed: &ParsedContract) -> syn::Result<TokenStream> {
    let has_custom_types = parsed
        .methods
        .iter()
        .flat_map(|method| {
            method
                .signature
                .inputs
                .iter()
                .chain(method.signature.outputs.iter())
        })
        .any(SolType::has_custom_types);

    let sol_encode_import = if has_custom_types {
        quote! {
            use ::pvm_contract_types::SolEncode;
        }
    } else {
        quote! {}
    };

    let constructor_entry = if parsed.has_constructor {
        let constructor_input_entries: Vec<TokenStream> = parsed
            .constructor_inputs
            .iter()
            .map(|(name, sol_type)| {
                let name_str = name.to_string();
                let type_name_expr = generate_sol_type_name_expr(sol_type)?;
                Ok(quote! {
                    if !__first_ctor_input {
                        __abi.push(',');
                    } else {
                        __first_ctor_input = false;
                    }
                    __abi.push_str("{\"name\":\"");
                    __abi.push_str(#name_str);
                    __abi.push_str("\",\"type\":\"");
                    __abi.push_str(&#type_name_expr);
                    __abi.push_str("\"}");
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;

        quote! {
            if !__first_item {
                __abi.push(',');
            } else {
                __first_item = false;
            }
            __abi.push_str("{\"type\":\"constructor\",\"inputs\":[");
            let mut __first_ctor_input = true;
            #(#constructor_input_entries)*
            __abi.push_str("]}");
        }
    } else {
        quote! {}
    };

    let method_entries: Vec<TokenStream> = parsed
        .methods
        .iter()
        .map(generate_method_entry)
        .collect::<syn::Result<Vec<_>>>()?;

    // Emit error ABI entries by calling error_signatures() on each error type.
    // This works for both single SolError types and sol_revert_enum! enums.
    let error_entries: Vec<TokenStream> = parsed
        .error_types
        .iter()
        .map(|err_ty| {
            quote! {
                for __sig in <#err_ty as ::pvm_contract_types::SolRevert>::error_signatures() {
                    if __seen_errors.contains(__sig) {
                        continue;
                    }
                    let Some(__paren) = __sig.find('(') else { continue; };
                    if !__sig.ends_with(')') { continue; }
                    __seen_errors.push(__sig);
                    if !__first_item {
                        __abi.push(',');
                    } else {
                        __first_item = false;
                    }
                    let __err_name = &__sig[..__paren];
                    let __params_str = &__sig[__paren + 1..__sig.len() - 1];
                    __abi.push_str("{\"type\":\"error\",\"name\":\"");
                    __abi.push_str(__err_name);
                    __abi.push_str("\",\"inputs\":[");
                    if !__params_str.is_empty() {
                        let mut __first_param = true;
                        for __param_type in __split_params(__params_str) {
                            if !__first_param {
                                __abi.push(',');
                            } else {
                                __first_param = false;
                            }
                            __abi.push_str("{\"name\":\"\",\"type\":\"");
                            __abi.push_str(__param_type);
                            __abi.push_str("\"}");
                        }
                    }
                    __abi.push_str("]}");
                }
            }
        })
        .collect();

    let split_params_helper = if !parsed.error_types.is_empty() {
        quote! {
            /// Split a comma-separated parameter string respecting parenthesis nesting.
            fn __split_params(s: &str) -> ::std::vec::Vec<&str> {
                let mut params = ::std::vec::Vec::new();
                let mut depth = 0usize;
                let mut start = 0;
                for (i, ch) in s.char_indices() {
                    match ch {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        ',' if depth == 0 => {
                            params.push(s[start..i].trim());
                            start = i + 1;
                        }
                        _ => {}
                    }
                }
                let last = s[start..].trim();
                if !last.is_empty() {
                    params.push(last);
                }
                params
            }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #[cfg(feature = "abi-gen")]
        fn main() {
            #sol_encode_import
            #split_params_helper

            let mut __abi = ::std::string::String::from("[");
            let mut __first_item = true;
            let mut __seen_errors = ::std::vec::Vec::<&str>::new();

            #constructor_entry

            #(#method_entries)*

            #(#error_entries)*

            __abi.push(']');
            ::std::println!("{}", __abi);
        }
    })
}

fn generate_method_entry(method: &MethodInfo) -> syn::Result<TokenStream> {
    let method_name = method.signature.name.clone();

    let input_entries: Vec<TokenStream> = method
        .signature
        .inputs
        .iter()
        .enumerate()
        .map(|(index, sol_type)| generate_input_entry(method, index, sol_type))
        .collect::<syn::Result<Vec<_>>>()?;

    let output_entries: Vec<TokenStream> = method
        .signature
        .outputs
        .iter()
        .map(generate_output_entry)
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        if !__first_item {
            __abi.push(',');
        } else {
            __first_item = false;
        }

        __abi.push_str("{\"type\":\"function\",\"name\":\"");
        __abi.push_str(#method_name);
        __abi.push_str("\",\"inputs\":[");

        let mut __first_input = true;
        #(#input_entries)*

        __abi.push_str("],\"outputs\":[");

        let mut __first_output = true;
        #(#output_entries)*

        __abi.push_str("]}");
    })
}

fn generate_input_entry(
    method: &MethodInfo,
    index: usize,
    sol_type: &SolType,
) -> syn::Result<TokenStream> {
    let param_name = method
        .param_names
        .get(index)
        .ok_or_else(|| {
            syn::Error::new_spanned(
                &method.fn_name,
                format!("Missing parameter name for input index {index}"),
            )
        })?
        .to_string();

    let type_name_expr = generate_sol_type_name_expr(sol_type)?;

    Ok(quote! {
        if !__first_input {
            __abi.push(',');
        } else {
            __first_input = false;
        }

        __abi.push_str("{\"name\":\"");
        __abi.push_str(#param_name);
        __abi.push_str("\",\"type\":\"");
        __abi.push_str(&#type_name_expr);
        __abi.push_str("\"}");
    })
}

fn generate_output_entry(sol_type: &SolType) -> syn::Result<TokenStream> {
    let type_name_expr = generate_sol_type_name_expr(sol_type)?;

    Ok(quote! {
        if !__first_output {
            __abi.push(',');
        } else {
            __first_output = false;
        }

        __abi.push_str("{\"name\":\"\",\"type\":\"");
        __abi.push_str(&#type_name_expr);
        __abi.push_str("\"}");
    })
}

fn generate_sol_type_name_expr(sol_type: &SolType) -> syn::Result<TokenStream> {
    if !sol_type.has_custom_types() {
        let canonical_name = sol_type.canonical_name();
        return Ok(quote! {
            ::std::string::String::from(#canonical_name)
        });
    }

    match sol_type {
        SolType::Custom(name) => {
            let ty = syn::parse_str::<syn::Type>(name).map_err(|error| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("Failed to parse custom SolType `{name}`: {error}"),
                )
            })?;

            Ok(quote! {
                ::std::string::String::from(<#ty as ::pvm_contract_types::SolEncode>::SOL_NAME)
            })
        }
        SolType::Array(inner) => {
            let inner_expr = generate_sol_type_name_expr(inner)?;
            Ok(quote! {{
                let mut __type_name = #inner_expr;
                __type_name.push_str("[]");
                __type_name
            }})
        }
        SolType::FixedArray(inner, size) => {
            let inner_expr = generate_sol_type_name_expr(inner)?;
            let size = size.to_string();
            Ok(quote! {{
                let mut __type_name = #inner_expr;
                __type_name.push('[');
                __type_name.push_str(#size);
                __type_name.push(']');
                __type_name
            }})
        }
        SolType::Tuple(types) => {
            let inner_exprs: Vec<TokenStream> = types
                .iter()
                .map(generate_sol_type_name_expr)
                .collect::<syn::Result<Vec<_>>>()?;

            Ok(quote! {{
                let mut __type_name = ::std::string::String::from("(");
                let mut __first_tuple_item = true;
                #(
                    if !__first_tuple_item {
                        __type_name.push(',');
                    } else {
                        __first_tuple_item = false;
                    }
                    __type_name.push_str(&(#inner_exprs));
                )*
                __type_name.push(')');
                __type_name
            }})
        }
        _ => {
            let canonical_name = sol_type.canonical_name();
            Ok(quote! {
                ::std::string::String::from(#canonical_name)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_empty_stream_for_sol_path_contract() {
        let parsed = ParsedContract {
            mod_name: syn::parse_str("contract").unwrap(),
            methods: vec![],
            has_constructor: false,
            has_fallback: false,
            constructor_name: None,
            constructor_returns_result: false,
            constructor_inputs: vec![],
            fallback_name: None,
            error_types: vec![],
        };

        assert!(generate_abi_gen_main(&parsed, true).is_empty());
    }

    #[test]
    fn error_types_appear_in_abi_gen_output() {
        let parsed = ParsedContract {
            mod_name: syn::parse_str("contract").unwrap(),
            methods: vec![],
            has_constructor: false,
            has_fallback: false,
            constructor_name: None,
            constructor_returns_result: false,
            constructor_inputs: vec![],
            fallback_name: None,
            error_types: vec![syn::parse_str("TokenError").unwrap()],
        };

        let output = generate_abi_gen_main(&parsed, false).to_string();

        assert!(
            output.contains("SolRevert"),
            "Should reference SolRevert trait: {output}"
        );
        assert!(
            output.contains("error_signatures"),
            "Should call error_signatures(): {output}"
        );
        assert!(
            output.contains(r#"\"type\":\"error\""#),
            "Should emit error ABI entry: {output}"
        );
    }
}
