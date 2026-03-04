#![cfg_attr(not(feature = "abi-gen"), no_main, no_std)]

use ruint::aliases::U256;

#[pvm_contract_macros::contract("ReturnValues.sol", allocator = "pico")]
mod return_values {
    use super::*;
    use pvm_contract_types::Address;

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
    pub fn get_pair() -> (U256, bool) {
        (U256::from(42u64), true)
    }

    #[pvm_contract_macros::method]
    pub fn get_triple() -> (U256, Address, bool) {
        let addr = Address::from([0xABu8; 20]);
        (U256::from(123u64), addr, false)
    }

    #[pvm_contract_macros::method]
    pub fn identity(val: U256) -> U256 {
        val
    }

    #[pvm_contract_macros::fallback]
    pub fn fallback() -> Result<(), Error> {
        Ok(())
    }
}
