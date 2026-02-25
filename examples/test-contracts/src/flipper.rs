#![cfg_attr(not(feature = "abi-gen"), no_main)]
#![cfg_attr(not(feature = "abi-gen"), no_std)]

use pallet_revive_uapi::{HostFnImpl as api, StorageFlags};

#[pvm_contract_macros::contract("Flipper.sol", allocator = "pico")]
mod flipper {
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

    const STORAGE_KEY: [u8; 32] = [0u8; 32];

    #[pvm_contract_macros::constructor]
    pub fn new() -> Result<(), Error> {
        // Initialize to false (0)
        api::set_storage(StorageFlags::empty(), &STORAGE_KEY, &[0u8; 32]);
        Ok(())
    }

    #[pvm_contract_macros::method]
    pub fn flip() {
        let current = read_value();
        let new_val = if current { 0u8 } else { 1u8 };
        let mut buf = [0u8; 32];
        buf[31] = new_val;
        api::set_storage(StorageFlags::empty(), &STORAGE_KEY, &buf);
    }

    #[pvm_contract_macros::method]
    pub fn get() -> bool {
        read_value()
    }

    #[pvm_contract_macros::fallback]
    pub fn fallback() -> Result<(), Error> {
        Ok(())
    }

    fn read_value() -> bool {
        let mut buf = [0u8; 32];
        let mut out = &mut buf[..];
        match api::get_storage(StorageFlags::empty(), &STORAGE_KEY, &mut out) {
            Ok(_) => buf[31] != 0,
            Err(_) => false,
        }
    }
}
