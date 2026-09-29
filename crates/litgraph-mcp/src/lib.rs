// SPDX-License-Identifier: GPL-3.0-or-later
//! `litgraph-mcp` — a stdio [MCP](https://modelcontextprotocol.io) server
//! that is a thin wrapper over [`litgraph::api::handle`]: the same one
//! door, one contract as the CLI and the library crate (see
//! `docs/ARCHITECTURE.md` commitment 1 in the litgraph repo).
//!
//! - Every engine [`litgraph::api::Op`] becomes an MCP tool. Each tool's
//!   input schema is sliced directly out of the engine's own `schemars`
//!   schema for [`litgraph::api::Request`] (see [`ops`]), so a tool schema
//!   cannot drift from what the engine actually accepts.
//! - A generic `litgraph` tool takes a raw [`litgraph::api::Request`]
//!   verbatim, for anything not (yet) exposed as its own tool.
//! - Packs, `links.json`, and the manual are MCP resources under
//!   `litgraph://…` URIs (see [`resources`]).
//! - Responses are always the same JSON envelope
//!   ([`litgraph::api::Response`]); an engine error (`ok: false`) becomes
//!   an MCP tool error (`isError: true`) carrying that same envelope as
//!   structured content. The server itself never panics: parse/dispatch
//!   failures become `ok: false` responses, exactly as `litgraph::api::handle_json`
//!   already guarantees for the CLI.

mod ops;
mod resources;
mod server;

pub use ops::OP_NAMES;
pub use server::LitgraphServer;
