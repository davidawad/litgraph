// SPDX-License-Identifier: GPL-3.0-or-later
//! `litgraph` — configure, submit and process litigation scenarios. JSON in, JSON out.
//!
//! ```text
//! litgraph describe                         capabilities, metrics, variables (start here)
//! litgraph schema <request|response|scenario|pack|links>   JSON Schema of a document
//! litgraph validate <file|->                check a pack, links, scenario or request
//! litgraph q '<request json>' | -          run any request
//! litgraph run <request.json>              run a request file
//! litgraph <op> [--packs a,b] [--scenario f.json] [--set k=v]... [--arg k=v]...
//! ```
//!
//! Exit status: 0 ok, 2 the request ran and failed (`ok: false` / invalid),
//! 1 usage or I/O error.

use std::io::Read as _;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Parser;
use litgraph::api::{self, Catalog};
use serde_json::{json, Map, Value};

#[derive(Parser)]
#[command(
    name = "litgraph",
    version,
    about = "Litigation procedure graphs as stochastic games: configure, submit and process scenarios. JSON in, JSON out.",
    after_help = "Ops: describe packs lint validate graph metric explain solve chain simulate path pareto sweep tornado structure compare batch.\nRun `litgraph describe` for the full machine-readable manual."
)]
struct Cli {
    /// `q`, `run`, `validate`, `schema`, or an op name.
    command: String,
    /// Request JSON / file for `q` and `run`, file for `validate`, kind for `schema` (`-` = stdin).
    input: Option<String>,
    /// Comma-separated pack ids (default: all).
    #[arg(long)]
    packs: Option<String>,
    /// Scenario JSON file, or inline JSON.
    #[arg(long)]
    scenario: Option<String>,
    /// Scenario parameter, e.g. `--set rate=900` (repeatable).
    #[arg(long = "set", value_name = "K=V")]
    set: Vec<String>,
    /// Op argument, e.g. `--arg node=answer-due --arg metrics=dollars,elapsed` (repeatable).
    #[arg(long = "arg", value_name = "K=V")]
    args: Vec<String>,
    /// Document kind for `validate` (default: detected).
    #[arg(long)]
    kind: Option<String>,
    /// Packs directory (default: `$LITGRAPH_PACKS`, else the packs built into this binary).
    #[arg(long)]
    packs_dir: Option<String>,
    /// Do not apply links.json.
    #[arg(long)]
    no_links: bool,
    /// Compact (single-line) output.
    #[arg(long)]
    compact: bool,
}

fn read_input(src: Option<&str>) -> Result<String> {
    match src {
        None | Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
        Some(s) if s.trim_start().starts_with('{') => Ok(s.to_string()),
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("reading {path}")),
    }
}

fn scalar(v: &str) -> Value {
    if let Ok(x) = v.parse::<f64>() {
        return json!(x);
    }
    match v {
        "true" => json!(true),
        "false" => json!(false),
        _ if v.starts_with('{') || v.starts_with('[') => serde_json::from_str(v).unwrap_or_else(|_| json!(v)),
        _ => json!(v),
    }
}

fn kv(s: &str, flag: &str) -> Result<(String, String)> {
    let (k, v) = s.split_once('=').with_context(|| format!("{flag} expects K=V, got `{s}`"))?;
    Ok((k.to_string(), v.to_string()))
}

/// Build a request from shorthand flags.
fn shorthand(cli: &Cli) -> Result<Value> {
    let mut scenario: Value = match &cli.scenario {
        Some(s) => serde_json::from_str(&read_input(Some(s))?).context("scenario JSON")?,
        None => json!({}),
    };
    for s in &cli.set {
        let (k, v) = kv(s, "--set")?;
        let Ok(x) = v.parse::<f64>() else { bail!("--set {k}: `{v}` is not a number (params are numeric)") };
        scenario["params"][k] = json!(x);
    }
    let mut op = Map::new();
    op.insert("op".into(), json!(cli.command));
    if let Some(i) = &cli.input {
        bail!("unexpected argument `{i}` for op `{}`; pass op arguments as --arg k=v", cli.command);
    }
    for a in &cli.args {
        let (k, v) = kv(a, "--arg")?;
        op.insert(k, scalar(&v));
    }
    let packs: Vec<&str> = cli.packs.as_deref().map(|p| p.split(',').collect()).unwrap_or_default();
    Ok(json!({ "packs": packs, "links": !cli.no_links, "scenario": scenario, "op": op }))
}

fn emit(v: &impl serde::Serialize, compact: bool) -> Result<()> {
    let s = if compact { serde_json::to_string(v)? } else { serde_json::to_string_pretty(v)? };
    println!("{s}");
    Ok(())
}

fn run(cli: &Cli) -> Result<bool> {
    let catalog = match &cli.packs_dir {
        Some(d) => Catalog::load(std::path::Path::new(d)),
        None => Catalog::default_source(),
    }
    .context("loading packs")?;
    match cli.command.as_str() {
        "schema" => {
            let kind = cli.input.as_deref().unwrap_or("request");
            emit(&api::schema(kind)?, cli.compact)?;
            Ok(true)
        }
        "validate" => {
            let doc: Value = serde_json::from_str(&read_input(cli.input.as_deref())?).context("document is not JSON")?;
            let v = api::validate(&doc, cli.kind.as_deref(), &catalog);
            emit(&v, cli.compact)?;
            Ok(v.valid)
        }
        "q" | "run" => {
            let resp = api::handle_json(&read_input(cli.input.as_deref())?, &catalog);
            emit(&resp, cli.compact)?;
            Ok(resp.ok)
        }
        _ => {
            let resp = api::handle_json(&shorthand(cli)?.to_string(), &catalog);
            emit(&resp, cli.compact)?;
            Ok(resp.ok)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(2),
        Err(e) => {
            eprintln!("litgraph: {e:#}");
            ExitCode::from(1)
        }
    }
}
