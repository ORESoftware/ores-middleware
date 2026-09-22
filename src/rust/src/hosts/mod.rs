//! Host adapters for isolated middleware execution.
//!
//! Host adapters own process/provider lifecycle. Policy semantics remain in
//! `MiddlewareStack`; request/finish data crosses the provider-neutral
//! `host_abi` contract.

pub mod edge_minimal_lifecycle;
pub mod edge_minimal_local;
pub mod local;
