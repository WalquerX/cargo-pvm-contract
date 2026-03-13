# Builder DSL for PVM Smart Contracts

A non-macro alternative to `#[contract]`. You wire up dispatch manually using `ContractBuilder`, with no proc macros and full explicit control over entry points and method routing.

## Basic Usage

```rust,ignore
#![no_main]
#![no_std]

use pallet_revive_uapi::{HostFn as _, HostFnImpl, ReturnFlags, StorageFlags};
use pvm_contract_builder_dsl::{ContractBuilder, solidity_selector};
use pvm_contract_types::{SolEncode, StaticEncodedLen};
use ruint::aliases::U256;

use pallet_revive_uapi::HostFnImpl as api;

const GET_VALUE: [u8; 4] = solidity_selector("getValue()");
const INCREMENT: [u8; 4] = solidity_selector("increment()");
const STORAGE_KEY: [u8; 32] = [0u8; 32];

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::arch::asm!("unimp"); core::hint::unreachable_unchecked() }
}

#[unsafe(no_mangle)]
#[polkavm_derive::polkavm_export]
pub extern "C" fn deploy() {}

#[unsafe(no_mangle)]
#[polkavm_derive::polkavm_export]
pub extern "C" fn call() {
    ContractBuilder::new()
        .method(GET_VALUE, get_value_handler)
        .method(INCREMENT, increment_handler)
        .dispatch::<HostFnImpl, 256>()
}

fn load_counter() -> U256 {
    let mut buf = [0u8; 32];
    let mut slice = &mut buf[..];
    match api::get_storage(StorageFlags::empty(), &STORAGE_KEY, &mut slice) {
        Ok(_) => U256::from_be_bytes::<32>(buf),
        Err(_) => U256::ZERO,
    }
}

fn get_value_handler(_input: &[u8]) {
    let result = load_counter();
    let mut buf = [0u8; <U256 as StaticEncodedLen>::ENCODED_SIZE];
    result.encode_to(&mut buf);
    HostFnImpl::return_value(ReturnFlags::empty(), &buf);
}

fn increment_handler(_input: &[u8]) {
    let value = load_counter() + U256::from(1);
    api::set_storage(StorageFlags::empty(), &STORAGE_KEY, &value.to_be_bytes::<32>());
    HostFnImpl::return_value(ReturnFlags::empty(), &[]);
}
```

## Contract Entry Points

Every PVM contract must export two `extern "C"` functions:

```text
deploy()  — called once during contract instantiation (constructor)
call()    — called on every subsequent interaction
```

With the DSL you define these explicitly. Inside `call()`, `ContractBuilder::dispatch` reads calldata, extracts the 4-byte selector, and routes to the matching method handler.

## Storage

There is no storage abstraction. You interact with the host directly:

```rust,ignore
use pallet_revive_uapi::{HostFnImpl as api, StorageFlags};

// Write a value
api::set_storage(StorageFlags::empty(), &key, &value_bytes);

// Read a value
let mut buf = [0u8; 32];
let mut slice = &mut buf[..];
match api::get_storage(StorageFlags::empty(), &key, &mut slice) {
    Ok(_) => { /* buf contains the value */ }
    Err(_) => { /* key not found */ }
}
```

## Events

Also manual — construct topic arrays and call the host:

```rust,ignore
// keccak256("Incremented(uint256)")
const INCREMENTED_EVENT_SIG: [u8; 32] = [
    0xe4, 0x8d, 0x01, 0x33, 0xf3, 0xb5, 0xf8, 0x87,
    0x0a, 0x62, 0xab, 0x1a, 0xd7, 0x0b, 0x7e, 0x6c,
    0x5a, 0x9e, 0x79, 0x43, 0xa8, 0x6c, 0x28, 0xd6,
    0x21, 0x67, 0xf2, 0x97, 0x59, 0x92, 0xd5, 0x0a,
];

fn emit_incremented(new_value: U256) {
    let topics = [INCREMENTED_EVENT_SIG];
    let data = new_value.to_be_bytes::<32>();
    api::deposit_event(&topics, &data);
}
```
