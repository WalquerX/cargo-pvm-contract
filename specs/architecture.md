# Architecture & Usage Guide

## What This Project Does

`cargo-pvm-contract` is a toolchain for writing Rust smart contracts that compile to PolkaVM bytecode and run on Polkadot's `pallet-revive`. It provides scaffolding, proc macros, ABI encoding, and a build pipeline that produces `.polkavm` binaries with `.abi.json` metadata.

Contracts use the **Ethereum ABI** (Keccak-256 selectors, Solidity-compatible encoding), so they can be called by the same tooling used for EVM contracts (ethers.js, cast, etc.).

## Workspace Crates

```
cargo-pvm-contract/
├── cargo-pvm-contract          CLI tool — scaffolds new contract projects
├── cargo-pvm-contract-builder  Build helper — invoked from build.rs, links PolkaVM bytecode + emits ABI JSON
├── pvm-contract-macros         Proc macros — #[contract], #[method], #[constructor], #[fallback], #[derive(SolType)]
├── pvm-contract-types          ABI traits — SolEncode / SolDecode, no_std compatible
├── pvm-contract-builder-dsl    Builder DSL — non-macro alternative (ContractBuilder)
├── pvm-bump-allocator          Bump allocator — simple no-dealloc heap for contract execution
└── pvm-contract-benchmarks     Benchmarks — binary size comparison tool
```

## Two API Styles

### 1. Proc Macro

Annotate a module with `#[contract]` and functions with `#[method]`. The macro generates entry points, calldata dispatch, and ABI encoding automatically.

```rust
#![no_main]
#![no_std]

use pallet_revive_uapi::{HostFnImpl as api, StorageFlags};
use ruint::aliases::U256;

#[pvm_contract_macros::contract("Counter.sol")]
mod counter {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {}

    impl AsRef<[u8]> for Error {
        fn as_ref(&self) -> &[u8] { match *self {} }
    }

    #[pvm_contract_macros::constructor]
    pub fn new() -> Result<(), Error> { Ok(()) }

    #[pvm_contract_macros::fallback]
    pub fn fallback() -> Result<(), Error> { Ok(()) }

    #[pvm_contract_macros::method]
    pub fn get_value() -> U256 {
        let key = [0u8; 32];
        let mut buf = [0u8; 32];
        let mut slice = &mut buf[..];
        match api::get_storage(StorageFlags::empty(), &key, &mut slice) {
            Ok(_) => U256::from_be_bytes::<32>(buf),
            Err(_) => U256::ZERO,
        }
    }

    #[pvm_contract_macros::method]
    pub fn increment() {
        let value = get_value() + U256::from(1);
        let key = [0u8; 32];
        api::set_storage(StorageFlags::empty(), &key, &value.to_be_bytes::<32>());
    }
}
```

The macro reads the `.sol` interface to compute Keccak-256 selectors. The Solidity file is only an interface (no implementation):

```solidity
// Counter.sol
interface Counter {
    function getValue() external view returns (uint256);
    function increment() external;
}
```

**Without a .sol file** — selectors are inferred from Rust function signatures. Rust `snake_case` names are converted to `camelCase` for the Solidity signature:

```rust
#[pvm_contract_macros::contract]
mod counter {
    #[pvm_contract_macros::method]
    pub fn get_value() -> U256 { ... }
    // → selector for "getValue()" = 0xff2551a1
}
```

### 2. Builder DSL (explicit control)

No proc macros. You wire up dispatch manually using `ContractBuilder`:

```rust
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

## Allocator Options

Contracts run in `no_std`. If you need heap allocation (`Vec`, `String`), you must choose an allocator.

### No Allocator (default)

Stack-only. Calldata is read into a fixed-size buffer. Only static return types allowed. Smallest binary size.

```rust
#[pvm_contract_macros::contract("Counter.sol", buffer = 256)]
mod counter { ... }
```

### Bump Allocator

Simple bump allocator from `pvm-bump-allocator`. Never frees memory (fine for short-lived contract calls). Based on ink!'s allocator design.

```rust
#[pvm_contract_macros::contract("Counter.sol", allocator = "bump")]
mod counter { ... }

// Custom heap size (default 1024 bytes):
#[pvm_contract_macros::contract("Counter.sol", allocator = "bump", allocator_size = 4096)]
mod counter { ... }
```

### Picoalloc

Third-party allocator with actual free support. Slightly larger binary.

```rust
#[pvm_contract_macros::contract("Counter.sol", allocator = "pico", allocator_size = 2048)]
mod counter { ... }
```

## Contract Anatomy

Every PVM contract has two entry points:

```
deploy()  — called once during contract instantiation (constructor)
call()    — called on every subsequent interaction
```

The `#[contract]` macro generates both. Inside `call()`, it:

1. Reads calldata from the host via `HostFnImpl::call_data_copy`
2. Extracts the 4-byte selector from calldata[0..4]
3. Matches the selector against registered methods
4. Decodes parameters using `SolDecode`
5. Calls your function
6. Encodes the return value using `SolEncode`
7. Returns to the caller via `HostFnImpl::return_value`

If no selector matches, the `#[fallback]` handler runs.

## Storage

There is no storage abstraction. You interact with the host directly:

```rust
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

```rust
// keccak256("Incremented(uint256)")
const INCREMENTED_EVENT_SIG: [u8; 32] = [
    0xe4, 0x8d, 0x01, 0x33, 0xf3, 0xb5, 0xf8, 0x87,
    0x0a, 0x62, 0xab, 0x1a, 0xd7, 0x0b, 0x7e, 0x6c,
    0x5a, 0x9e, 0x79, 0x43, 0xa8, 0x6c, 0x28, 0xd6,
    0x21, 0x67, 0xf2, 0x97, 0x59, 0x92, 0xd5, 0x0a,
];

fn emit_incremented(new_value: U256) {
    // topics[0] = event signature hash
    let topics = [INCREMENTED_EVENT_SIG];
    let data = new_value.to_be_bytes::<32>();
    api::deposit_event(&topics, &data);
}
```

## Error Handling

Methods can return `Result<T, Error>` or plain `T`:

```rust
// Fallible — Err reverts the transaction with the error message
#[pvm_contract_macros::method]
pub fn decrement() -> Result<(), Error> {
    let value = get_value();
    if value == U256::ZERO {
        return Err(Error::Underflow);
    }
    let key = [0u8; 32];
    api::set_storage(StorageFlags::empty(), &key, &(value - U256::from(1)).to_be_bytes::<32>());
    Ok(())
}

// Infallible — always succeeds
#[pvm_contract_macros::method]
pub fn get_value() -> U256 {
    // read from storage...
}
```

The `Error` enum must implement `AsRef<[u8]>` so the macro can serialize it on revert:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Underflow,
}

impl AsRef<[u8]> for Error {
    fn as_ref(&self) -> &[u8] {
        match *self {
            Error::Underflow => b"Underflow",
        }
    }
}
```

## Custom Types

Use `#[derive(SolType)]` to make structs usable as method parameters or return types:

```rust
#[derive(pvm_contract_macros::SolType)]
pub struct Point {
    pub x: U256,
    pub y: U256,
}

#[pvm_contract_macros::method]
pub fn set_point(point: Point) { ... }

#[pvm_contract_macros::method]
pub fn get_point() -> Point {
    Point { x: U256::from(1), y: U256::from(2) }
}
```

This generates `SolEncode`, `SolDecode`, and `StaticEncodedLen` implementations. The ABI encoding follows Solidity tuple layout. See [specs/abi.md](abi.md) for encoding details.

## ABI Generation

When using the proc macro style, the build system automatically generates a `.abi.json` file:

```
target/counter.release.polkavm      — deployable bytecode
target/counter.release.abi.json     — Ethereum-compatible ABI JSON
```

The ABI JSON follows the standard Ethereum ABI format, so it can be used with viem, ethers.js, or any tool that consumes Solidity ABIs.

With the DSL style, ABI generation is skipped.

## Scaffolding a New Project

See [build.md](build.md) for scaffolding instructions, build commands, and generated project structure.

## Type Mapping: Solidity to Rust

See [abi.md](abi.md#type-mapping-solidity--rust) for the full type mapping table and encoding details.

## Host Functions ([pallet-revive-uapi](https://github.com/paritytech/polkadot-sdk/blob/master/substrate/frame/revive/uapi/src/host.rs))

Contracts communicate with the runtime through `pallet_revive_uapi::HostFnImpl`. This provides syscall-like functions for storage access, reading calldata, returning values, querying account/block info, calling other contracts, emitting events, and hashing.

## Deploying and Interacting with Contracts

See [deployment.md](deployment.md) for full instructions on deploying `.polkavm` bytecode and interacting with contracts using Foundry (`cast`).
