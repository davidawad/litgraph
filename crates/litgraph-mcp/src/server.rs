// SPDX-License-Identifier: GPL-3.0-or-later
//! The MCP [`ServerHandler`]: wires [`crate::ops`] and [`crate::resources`]
//! into `tools/list`, `tools/call`, `resources/list`, and
//! `resources/read`. Tools and resources are dispatched by hand (rather
//! than the `#[tool_router]`/`#[resource_router]` macros) because the
//! tool set is generated at runtime from the engine's own `Op` schema.

use litgraph::api::Catalog;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, ServerCapabilities,
    ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::json;

use crate::{ops, resources};

/// The MCP server: a loaded [`Catalog`] plus the tool/resource surface in
/// [`crate::ops`] and [`crate::resources`]. A thin wrapper over
/// [`litgraph::api::handle`] end to end -- this type holds no state of its
/// own beyond the packs.
#[derive(Debug, Clone)]
pub struct LitgraphServer {
    catalog: Catalog,
}

impl LitgraphServer {
    /// Build a server over an already-loaded catalog (see
    /// `Catalog::default_source`/`Catalog::load` for honoring
    /// `LITGRAPH_PACKS`/`--packs-dir`).
    #[must_use]
    pub fn new(catalog: Catalog) -> Self {
        Self { catalog }
    }
}

impl ServerHandler for LitgraphServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(
            Implementation::new("litgraph-mcp", env!("CARGO_PKG_VERSION")).with_description(
                "Litigation procedure graphs as stochastic games, over MCP: one tool per \
                     engine op, packs/links.json/the manual as resources.",
            ),
        )
        .with_instructions(
            "Start with the `describe` tool (or the `litgraph://describe` resource) for \
                 the full manual: ops, scenario fields, metrics, variables, params. Every op \
                 tool takes `{packs?, links?, no_continuations?, scenario?, ...op fields}`; \
                 `packs` defaults to every pack. The `litgraph` tool takes a raw request \
                 `{packs, links, no_continuations, scenario, op}` verbatim -- use it for \
                 anything not (yet) exposed as its own tool. Every response is the same JSON \
                 envelope the CLI and library use ({ok, api_version, op, result|error, \
                 warnings, provenance, elapsed_ms}); an engine error (`ok: false`) comes back \
                 as a tool error carrying that same envelope as structured content.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(ops::tools(&self.catalog)))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.as_ref();
        let response = if name == ops::GENERIC_TOOL {
            ops::call_raw(&self.catalog, request.arguments)
        } else if ops::op_names().iter().any(|n| n == name) {
            ops::call_op(&self.catalog, name, request.arguments)
        } else {
            return Err(McpError::invalid_params(
                format!("unknown tool `{name}`"),
                None,
            ));
        };
        // Every response is `litgraph::api::Response` (`Serialize`), so
        // this can only fail for pathological non-UTF8-safe content that
        // the engine never produces; fall back rather than unwrap/panic.
        let value = serde_json::to_value(&response)
            .unwrap_or_else(|e| json!({ "ok": false, "error": e.to_string() }));
        let result = if response.ok {
            CallToolResult::structured(value)
        } else {
            CallToolResult::structured_error(value)
        };
        Ok(result.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(resources::list(
            &self.catalog,
        )))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        // No `litgraph://scenarios/<name>` templates yet -- see the module
        // doc on `crate::resources` and the follow-up bead for wiring them
        // in once the scenario library lands.
        Ok(ListResourceTemplatesResult::default())
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        match resources::read(&self.catalog, &request.uri) {
            Some(contents) => Ok(ReadResourceResult::new(contents).into()),
            None => Err(McpError::resource_not_found(
                format!("no resource at `{}`", request.uri),
                Some(json!({ "uri": request.uri })),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> LitgraphServer {
        LitgraphServer::new(Catalog::embedded().expect("embedded packs must load in tests"))
    }

    #[test]
    fn get_info_advertises_tools_and_resources() {
        let info = server().get_info();
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_some());
        assert!(info.instructions.is_some());
    }

    // `call_tool`/`list_tools`/`read_resource` need a live `RequestContext`
    // (peer, request id, protocol version), which only a real transport
    // constructs; they are exercised end to end over the stdio protocol in
    // `tests/mcp_protocol.rs` instead of built by hand here. The routing
    // logic they wrap (`ops::tools`, `ops::call_op`/`call_raw`,
    // `resources::list`/`read`) has its own unit tests in `ops.rs` and
    // `resources.rs`.
}
