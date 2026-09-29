// SPDX-License-Identifier: GPL-3.0-or-later
//! End-to-end protocol test: spawn the real, compiled `litgraph-mcp`
//! binary as a child process over stdio and drive it with an MCP client
//! -- `initialize`, `tools/list`, `tools/call solve`, `resources/read`.
//!
//! This is the only place that exercises `main.rs`'s own wiring (arg
//! parsing, `Catalog::default_source`, `ServiceExt::serve(stdio())`); the
//! tool/resource logic itself has direct unit tests in `src/ops.rs` and
//! `src/resources.rs`.

use anyhow::{bail, Context, Result};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Map, Value};
use tokio::process::Command;

fn object(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// Spawn `litgraph-mcp` and complete `initialize` over stdio.
async fn connect() -> Result<RunningService<RoleClient, ()>> {
    connect_with(|_| {}).await
}

/// Same as [`connect`], but with the child command configured first (extra
/// args, env vars) before it is spawned.
async fn connect_with(
    configure: impl FnOnce(&mut Command),
) -> Result<RunningService<RoleClient, ()>> {
    ().serve(TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_litgraph-mcp")).configure(configure),
    )?)
    .await
    .context("initializing the litgraph-mcp session")
}

fn structured(result: &CallToolResult) -> Result<Value> {
    result
        .structured_content
        .clone()
        .context("tool call had no structured content")
}

fn repo_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../{name}"));
    assert!(dir.is_dir(), "expected a {name}/ dir at {}", dir.display());
    dir
}

#[tokio::test]
async fn initialize_advertises_tools_and_resources() -> Result<()> {
    let client = connect().await?;
    let info = client
        .peer_info()
        .context("no peer info after initialize")?;
    assert!(
        info.capabilities.tools.is_some(),
        "server should advertise tools"
    );
    assert!(
        info.capabilities.resources.is_some(),
        "server should advertise resources"
    );
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn list_tools_includes_every_op_and_the_generic_tool() -> Result<()> {
    let client = connect().await?;
    let tools = client.list_all_tools().await?;
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in ["describe", "solve", "chain", "explain", "litgraph"] {
        assert!(
            names.contains(&expected),
            "missing tool `{expected}` in {names:?}"
        );
    }
    let solve = tools
        .iter()
        .find(|t| t.name == "solve")
        .context("no `solve` tool")?;
    let props = solve
        .input_schema
        .get("properties")
        .and_then(Value::as_object);
    assert!(
        props.is_some_and(|p| p.contains_key("packs") && p.contains_key("max_steps")),
        "solve tool schema missing expected fields: {:?}",
        solve.input_schema
    );
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn call_tool_solve_returns_the_engine_envelope() -> Result<()> {
    let client = connect().await?;
    let result = client
        .call_tool(
            CallToolRequestParams::new("solve")
                .with_arguments(object(json!({ "packs": ["cofc"] }))),
        )
        .await?;
    let envelope = structured(&result)?;
    assert_eq!(envelope["ok"], json!(true), "envelope: {envelope}");
    assert_eq!(envelope["op"], json!("solve"));
    assert!(envelope["result"]["value"].is_number());
    assert_ne!(result.is_error, Some(true));
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn call_tool_with_an_unknown_field_is_a_tool_error_not_a_panic() -> Result<()> {
    let client = connect().await?;
    let result = client
        .call_tool(
            CallToolRequestParams::new("solve")
                .with_arguments(object(json!({ "not_a_real_field": 1 }))),
        )
        .await?;
    assert_eq!(result.is_error, Some(true));
    let envelope = structured(&result)?;
    assert_eq!(envelope["ok"], json!(false));
    assert_eq!(envelope["error"]["code"], json!("parse"));
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn resources_list_and_read_links() -> Result<()> {
    let client = connect().await?;
    let resources = client.list_all_resources().await?;
    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(uris.contains(&"litgraph://describe"));
    assert!(uris.contains(&"litgraph://links"));
    assert!(
        uris.iter().any(|u| u.starts_with("litgraph://packs/")),
        "no pack resources in {uris:?}"
    );

    let read = client
        .read_resource(ReadResourceRequestParams::new("litgraph://links"))
        .await?;
    let ResourceContents::TextResourceContents { text, .. } = &read.contents[0] else {
        bail!(
            "expected text contents for litgraph://links, got {:?}",
            read.contents[0]
        );
    };
    assert!(
        serde_json::from_str::<Value>(text).is_ok(),
        "links.json resource is not valid JSON"
    );
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn resources_list_and_read_scenarios() -> Result<()> {
    let client = connect().await?;
    let resources = client.list_all_resources().await?;
    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(uris.contains(&"litgraph://scenarios"));
    assert!(
        uris.iter().any(|u| u.starts_with("litgraph://scenarios/")),
        "no per-scenario resources in {uris:?}"
    );

    let index = client
        .read_resource(ReadResourceRequestParams::new("litgraph://scenarios"))
        .await?;
    let ResourceContents::TextResourceContents { text, .. } = &index.contents[0] else {
        bail!(
            "expected text contents for litgraph://scenarios, got {:?}",
            index.contents[0]
        );
    };
    let listed: Value = serde_json::from_str(text)?;
    let first = listed["scenarios"]
        .as_array()
        .and_then(|xs| xs.first())
        .context("scenarios index had no entries")?;
    let id = first["id"].as_str().context("scenario entry had no id")?;

    let named = client
        .read_resource(ReadResourceRequestParams::new(format!(
            "litgraph://scenarios/{id}"
        )))
        .await?;
    let ResourceContents::TextResourceContents { text, .. } = &named.contents[0] else {
        bail!(
            "expected text contents for litgraph://scenarios/{id}, got {:?}",
            named.contents[0]
        );
    };
    let parsed: Value = serde_json::from_str(text)?;
    assert_eq!(parsed["id"], json!(id));
    assert!(
        parsed.get("scenario").is_some(),
        "expected the full engine Scenario embedded"
    );

    let missing = client
        .read_resource(ReadResourceRequestParams::new(
            "litgraph://scenarios/no-such-scenario",
        ))
        .await;
    assert!(
        missing.is_err(),
        "reading an unknown scenario id should be a protocol error"
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn packs_dir_flag_overrides_the_embedded_packs() -> Result<()> {
    let packs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
    assert!(
        packs_dir.is_dir(),
        "expected a packs/ dir at {}",
        packs_dir.display()
    );

    let client = connect_with(|cmd| {
        cmd.arg("--packs-dir").arg(&packs_dir);
    })
    .await?;
    let result = client
        .call_tool(CallToolRequestParams::new("describe"))
        .await?;
    let envelope = structured(&result)?;
    assert_eq!(
        envelope["result"]["packs_source"],
        json!(packs_dir.display().to_string())
    );
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn litgraph_packs_env_var_overrides_the_embedded_packs() -> Result<()> {
    let packs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
    assert!(
        packs_dir.is_dir(),
        "expected a packs/ dir at {}",
        packs_dir.display()
    );

    let client = connect_with(|cmd| {
        cmd.env("LITGRAPH_PACKS", &packs_dir);
    })
    .await?;
    let result = client
        .call_tool(CallToolRequestParams::new("describe"))
        .await?;
    let envelope = structured(&result)?;
    assert_eq!(
        envelope["result"]["packs_source"],
        json!(packs_dir.display().to_string())
    );
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn scenarios_dir_flag_overrides_the_embedded_scenarios() -> Result<()> {
    let scenarios_dir = repo_dir("scenarios");

    let client = connect_with(|cmd| {
        cmd.arg("--scenarios-dir").arg(&scenarios_dir);
    })
    .await?;
    let result = client
        .call_tool(CallToolRequestParams::new("describe"))
        .await?;
    let envelope = structured(&result)?;
    assert_eq!(
        envelope["result"]["scenarios_source"],
        json!(scenarios_dir.display().to_string())
    );
    // Packs still resolve to the embedded set -- the flags are independent.
    assert_eq!(envelope["result"]["packs_source"], json!("embedded"));
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn litgraph_scenarios_env_var_overrides_the_embedded_scenarios() -> Result<()> {
    let scenarios_dir = repo_dir("scenarios");

    let client = connect_with(|cmd| {
        cmd.env("LITGRAPH_SCENARIOS", &scenarios_dir);
    })
    .await?;
    let result = client
        .call_tool(CallToolRequestParams::new("describe"))
        .await?;
    let envelope = structured(&result)?;
    assert_eq!(
        envelope["result"]["scenarios_source"],
        json!(scenarios_dir.display().to_string())
    );
    client.cancel().await?;
    Ok(())
}
