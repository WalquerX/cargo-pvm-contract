#![cfg_attr(not(feature = "abi-gen"), no_main)]
#![cfg_attr(not(feature = "abi-gen"), no_std)]

use ruint::aliases::U256;

#[pvm_contract_macros::contract("CompositeTypes.sol", allocator = "pico")]
mod composite_types {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        Unexpected,
    }

    impl AsRef<[u8]> for Error {
        fn as_ref(&self) -> &[u8] {
            match *self {
                Error::Unexpected => b"Unexpected",
            }
        }
    }

    #[pvm_contract_macros::constructor]
    pub fn new() -> Result<(), Error> {
        Ok(())
    }

    #[pvm_contract_macros::method]
    pub fn sum_fixed_array(scores: [U256; 3]) -> U256 {
        scores[0].wrapping_add(scores[1]).wrapping_add(scores[2])
    }

    #[pvm_contract_macros::method]
    pub fn get_fixed_array() -> [U256; 3] {
        [U256::from(10), U256::from(20), U256::from(30)]
    }

    #[pvm_contract_macros::method]
    pub fn process_tuple(data: (U256, bool)) -> U256 {
        if data.1 { data.0 } else { U256::ZERO }
    }

    #[pvm_contract_macros::fallback]
    pub fn fallback() -> Result<(), Error> {
        Ok(())
    }
}
