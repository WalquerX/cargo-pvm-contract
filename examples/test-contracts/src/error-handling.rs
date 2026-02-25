#![cfg_attr(not(feature = "abi-gen"), no_main)]
#![cfg_attr(not(feature = "abi-gen"), no_std)]

use pallet_revive_uapi::{HostFnImpl as api, StorageFlags};
use ruint::aliases::U256;

#[pvm_contract_macros::contract("ErrorHandling.sol", allocator = "pico")]
mod error_handling {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        AlwaysReverts,
        ZeroNotAllowed,
    }

    impl AsRef<[u8]> for Error {
        fn as_ref(&self) -> &[u8] {
            match *self {
                Error::AlwaysReverts => b"AlwaysReverts",
                Error::ZeroNotAllowed => b"ZeroNotAllowed",
            }
        }
    }

    const GUARDED_KEY: [u8; 32] = [0u8; 32];

    #[pvm_contract_macros::constructor]
    pub fn new() -> Result<(), Error> {
        Ok(())
    }

    #[pvm_contract_macros::method]
    pub fn will_revert() -> Result<(), Error> {
        Err(Error::AlwaysReverts)
    }

    #[pvm_contract_macros::method]
    pub fn will_succeed() -> bool {
        true
    }

    #[pvm_contract_macros::method]
    pub fn set_guarded(val: U256) -> Result<(), Error> {
        if val == U256::ZERO {
            return Err(Error::ZeroNotAllowed)
        }
        api::set_storage(StorageFlags::empty(), &GUARDED_KEY, &val.to_be_bytes::<32>());
        Ok(())
    }

    #[pvm_contract_macros::method]
    pub fn get_guarded() -> U256 {
        let mut buf = [0u8; 32];
        let mut out = &mut buf[..];
        match api::get_storage(StorageFlags::empty(), &GUARDED_KEY, &mut out) {
            Ok(_) => U256::from_be_bytes::<32>(buf),
            Err(_) => U256::ZERO,
        }
    }

    #[pvm_contract_macros::fallback]
    pub fn fallback() -> Result<(), Error> {
        Ok(())
    }
}
