# Protocol

This folder holds the Tunnelcrust binary protocol. This is used for serializing and deserializing requests when transferred between server & clients.

## Get Started

> [!IMPORTANT]
> Ensure you have met the development prerequisites listed in the main [README](../README.md).

This is a library crate designed for internal use. It is defined as such as package in the `cargo.toml` files of both
server & client crates.

## Tests

The protocol has unit tests covering the individual components. You can run these using `cargo test`.