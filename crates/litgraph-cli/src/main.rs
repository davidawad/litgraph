//! `litgraph` — JSON in, JSON out.
//!
//!   litgraph describe                      capabilities, metrics, variables
//!   litgraph q '<request json>'            any request (or `-` for stdin)
//!   litgraph <op> [--packs a,b] [--set k=v]... [--scenario file.json] [--arg k=v]...
//!
//! Shorthand ops build the same request `q` takes, e.g.
//!   litgraph explain --packs frcp --arg node=answer-due --set rate=900

use anyhow::{bail, Context, Result};
use clap::Parser;
use litgraph::api::{self, Catalog, Request};
use serde_json::{json, Map, Value};

#[derive(Parser)]
#[command(
    name = "litgraph",
    about = "Litigation procedure graphs: solve, simulate, explore. JSON in/out."
)]
struct Cli {
    /// Operation (describe, packs, lint, graph, metric, explain, solve, chain, simulate,
    /// path, pareto, sweep, tornado, structure, compare, batch) or `q` for a raw request.
    op: String,
    /// Raw request JSON for `q` (or `-` for stdin).
    request: Option<String>,
    /// Comma-separated pack ids (default: all).
    #[arg(long)]
    packs: Option<String>,
    /// Scenario JSON file (or inline JSON).
    #[arg(long)]
    scenario: Option<String>,
    /// Scenario param, e.g. --set rate=900 (repeatable).
    #[arg(long = "set")]
    set: Vec<String>,
    /// Op argument, e.g. --arg node=answer-due --arg metrics=dollars,elapsed (repeatable).
    #[arg(long = "arg")]
    args: Vec<String>,
    /// Packs directory (default: $LITGRAPH_PACKS, nearest ./packs, or the repo's packs/).
    #[arg(long)]
    packs_dir: Option<String>,
    /// Do not apply packs/links.json.
    #[arg(long)]
    no_links: bool,
    /// Compact output (default pretty).
    #[arg(long)]
    compact: bool,
}

fn parse_scalar(v: &str) -> Value {
    if let Ok(x) = v.parse::<f64>() {
        return json!(x);
    }
    match v {
        "true" => json!(true),
        "false" => json!(false),
        _ if v.starts_with('{') || v.starts_with('[') => {
            serde_json::from_str(v).unwrap_or(json!(v))
        }
        _ if v.contains(',') => json!(v.split(',').collect::<Vec<_>>()),
        _ => json!(v),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = cli
        .packs_dir
        .clone()
        .map(Into::into)
        .unwrap_or_else(api::packs_dir);
    let catalog =
        Catalog::load(&dir).with_context(|| format!("loading packs from {}", dir.display()))?;

    let req: Request = if cli.op == "q" {
        let raw = match cli.request.as_deref() {
            Some("-") | None => std::io::read_to_string(std::io::stdin())?,
            Some(s) => s.to_string(),
        };
        serde_json::from_str(&raw).context("request JSON")?
    } else {
        let mut scenario: Value = match &cli.scenario {
            Some(s) if s.trim_start().starts_with('{') => serde_json::from_str(s)?,
            Some(path) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
            None => json!({}),
        };
        for kv in &cli.set {
            let (k, v) = kv.split_once('=').context("--set expects k=v")?;
            let Ok(x) = v.parse::<f64>() else {
                bail!("--set {k}: `{v}` is not a number (params are numeric)")
            };
            scenario["params"][k] = json!(x);
        }
        let mut op = Map::new();
        op.insert("op".into(), json!(cli.op));
        for kv in &cli.args {
            let (k, v) = kv.split_once('=').context("--arg expects k=v")?;
            op.insert(k.into(), parse_scalar(v));
        }
        json!({
            "packs": cli.packs.as_deref().map(|p| p.split(',').map(str::to_string).collect::<Vec<_>>()).unwrap_or_default(),
            "links": !cli.no_links,
            "scenario": scenario,
            "op": op,
        })
        .pipe(serde_json::from_value)
        .context("building request")?
    };
    let out = api::handle(&req, &catalog);
    let ok = out["ok"].as_bool().unwrap_or(false);
    println!(
        "{}",
        if cli.compact {
            serde_json::to_string(&out)?
        } else {
            serde_json::to_string_pretty(&out)?
        }
    );
    if !ok {
        std::process::exit(2);
    }
    Ok(())
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}
