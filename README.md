# Tunnelcrust

A tunneling application designed to route traffic without exposing inbound ports.

> [!WARNING]
> Tunnelcrust is **not** a production-ready application. I am not a network specialist or Rust expert in anyway. This is merely a hobby project to enhance both my Rust & networking knowledge.

## Repository structure

This repository is a cargo workspace with two packages:

- [Client](./client) - The local client used to connect to the tunnel server.
- [Server](./server) - A tunnel server implementation.

## Development Prerequisites

The [devcontainer](./devcontainer) can be used to replicate the intended dev environment. Documentation on devcontainers can be found on [VS Code](https://code.visualstudio.com/docs/devcontainers/containers). This also contains a set of extensions (if using VS Code) to assist with development.

- [Rust](https://www.rust-lang.org/tools/install) - 1.79.0 