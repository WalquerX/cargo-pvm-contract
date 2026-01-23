# cargo-pvm-contract

A cargo subcommand to build Rust contracts to PolkaVM bytecode.

This tool scaffolds Rust contract projects from Solidity interface files (`.sol`). It automates:
- Function selector generation
- Input/output encoding and decoding (ABI)

This tool is designed for building smart contracts in Rust using the low-level API provided by [pallet-revive-uapi](https://docs.rs/pallet-revive-uapi/latest/pallet_revive_uapi/). For a more high-level, user-friendly API, see [Ink!](https://use.ink/).

## Contract Generation

Contracts can be generated using two approaches:

- **High-level API**: Uses the `sol!` macro with automatic struct generation for type-safe contract development
- **Low-level API**: Provides more manual control over the contract implementation

To learn more, visit the [Rust Contract Template](https://github.com/paritytech/rust-contract-template).

## Installation

```bash
cargo install --force --locked cargo-pvm-contract
```

## Usage

Once installed, you can use it as a cargo subcommand:

```bash
cargo pvm-contract
```

This launches an interactive prompt to initialize a new contract project.
Just build the generated project with `cargo build`. The PolkaVM bytecode will be written to `target/<bin>.<profile>.polkavm`.


