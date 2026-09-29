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
    /// Comma-separated pack ids (default: all, or the named scenario's packs).
    #[arg(long)]
    packs: Option<String>,
    /// Scenario: a name from the library (`litgraph describe`), a JSON file, or inline JSON.
    #[arg(long)]
    scenario: Option<String>,
    /// Scenario parameter, e.g. `--set rate=900` (repeatable). Composes with
    /// a named `--scenario` via `extends`.
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
    /// Scenario library directory (default: `$LITGRAPH_SCENARIOS`, else the
    /// scenarios built into this binary).
    #[arg(long)]
    scenarios_dir: Option<String>,
    /// Calibration directory (default: `$LITGRAPH_CALIBRATION`, else the sets built into this
    /// binary); sets `$LITGRAPH_CALIBRATION` for this process so `scenario.calibration` picks it up.
    #[arg(long)]
    calibration_dir: Option<String>,
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
        _ if v.starts_with('{') || v.starts_with('[') => {
            serde_json::from_str(v).unwrap_or_else(|_| json!(v))
        }
        _ => json!(v),
    }
}

fn kv(s: &str, flag: &str) -> Result<(String, String)> {
    let (k, v) = s
        .split_once('=')
        .with_context(|| format!("{flag} expects K=V, got `{s}`"))?;
    Ok((k.to_string(), v.to_string()))
}

/// A `--scenario` value: inline JSON (`{...}`/`[...]`), an existing file
/// (read and parsed as JSON), or otherwise a bare name from the library.
fn scenario_arg(s: &str) -> Result<Value> {
    let t = s.trim_start();
    if t.starts_with('{') || t.starts_with('[') {
        return serde_json::from_str(s).context("scenario JSON");
    }
    if std::path::Path::new(s).is_file() {
        let text = std::fs::read_to_string(s).with_context(|| format!("reading {s}"))?;
        return serde_json::from_str(&text).context("scenario JSON file");
    }
    Ok(json!(s))
}

/// Build a request from shorthand flags.
fn shorthand(cli: &Cli) -> Result<Value> {
    let mut scenario: Value = match &cli.scenario {
        Some(s) => scenario_arg(s)?,
        None => json!({}),
    };
    if !cli.set.is_empty() {
        // A bare named scenario (`--scenario cofc-1498-patent-case`) becomes
        // `{"extends": "<name>"}` so `--set` composes onto it instead of
        // indexing into a JSON string.
        if let Some(name) = scenario.as_str() {
            scenario = json!({ "extends": name });
        }
        for s in &cli.set {
            let (k, v) = kv(s, "--set")?;
            let Ok(x) = v.parse::<f64>() else {
                bail!("--set {k}: `{v}` is not a number (params are numeric)")
            };
            scenario["params"][k] = json!(x);
        }
    }
    let mut op = Map::new();
    op.insert("op".into(), json!(cli.command));
    if let Some(i) = &cli.input {
        bail!(
            "unexpected argument `{i}` for op `{}`; pass op arguments as --arg k=v",
            cli.command
        );
    }
    for a in &cli.args {
        let (k, v) = kv(a, "--arg")?;
        op.insert(k, scalar(&v));
    }
    let packs: Vec<&str> = cli
        .packs
        .as_deref()
        .map(|p| p.split(',').collect())
        .unwrap_or_default();
    Ok(json!({ "packs": packs, "links": !cli.no_links, "scenario": scenario, "op": op }))
}

fn emit(v: &impl serde::Serialize, compact: bool) -> Result<()> {
    let s = if compact {
        serde_json::to_string(v)?
    } else {
        serde_json::to_string_pretty(v)?
    };
    println!("{s}");
    Ok(())
}

/// Resolve packs and the scenario library independently: `--packs-dir` /
/// `--scenarios-dir` each fall back to their own env var, else what's built
/// into this binary (`Catalog::default_source`'s precedent, applied to each
/// half separately so overriding one never disturbs the other).
fn load_catalog(cli: &Cli) -> Result<Catalog> {
    let catalog = match &cli.packs_dir {
        Some(d) => Catalog::load(std::path::Path::new(d)),
        None => Catalog::default_source(),
    }
    .context("loading packs")?;
    let scenarios_dir = cli
        .scenarios_dir
        .clone()
        .or_else(|| std::env::var("LITGRAPH_SCENARIOS").ok());
    match scenarios_dir {
        Some(d) => catalog.with_scenarios_dir(std::path::Path::new(&d)),
        None => catalog.with_embedded_scenarios(),
    }
    .context("loading scenarios")
}

fn run(cli: &Cli) -> Result<bool> {
    if let Some(d) = &cli.calibration_dir {
        std::env::set_var("LITGRAPH_CALIBRATION", d);
    }
    let catalog = load_catalog(cli)?;
    match cli.command.as_str() {
        "schema" => {
            let kind = cli.input.as_deref().unwrap_or("request");
            emit(&api::schema(kind)?, cli.compact)?;
            Ok(true)
        }
        "validate" => {
            let doc: Value = serde_json::from_str(&read_input(cli.input.as_deref())?)
                .context("document is not JSON")?;
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A minimal `Cli` with every flag at its default; override fields with
    /// struct-update syntax (`Cli { command: "chain".into(), ..cli() }`).
    fn cli() -> Cli {
        Cli {
            command: "describe".into(),
            input: None,
            packs: None,
            scenario: None,
            set: vec![],
            args: vec![],
            kind: None,
            packs_dir: None,
            scenarios_dir: None,
            calibration_dir: None,
            no_links: false,
            compact: false,
        }
    }

    fn packs_dir() -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packs")
            .display()
            .to_string()
    }

    fn scenarios_dir() -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scenarios")
            .display()
            .to_string()
    }

    #[test]
    fn scalar_reads_numbers_bools_json_and_falls_back_to_a_string() {
        assert_eq!(scalar("900"), json!(900.0));
        assert_eq!(scalar("true"), json!(true));
        assert_eq!(scalar("false"), json!(false));
        assert_eq!(scalar("[1,2]"), json!([1, 2]));
        assert_eq!(scalar("{not json"), json!("{not json"));
        assert_eq!(scalar("answer-due"), json!("answer-due"));
    }

    #[test]
    fn kv_splits_on_equals_and_errors_without_one() {
        assert_eq!(
            kv("rate=900", "--set").unwrap(),
            ("rate".into(), "900".into())
        );
        assert!(kv("norate", "--set").is_err());
    }

    #[test]
    fn scenario_arg_reads_inline_json_a_file_or_falls_back_to_a_bare_name() {
        assert_eq!(
            scenario_arg(r#"{"params":{"rate":9}}"#).unwrap(),
            json!({"params": {"rate": 9}})
        );
        assert_eq!(scenario_arg("[1]").unwrap(), json!([1]));
        assert!(scenario_arg("{not json").is_err());

        let file = std::env::temp_dir().join(format!(
            "litgraph-cli-test-scenario-arg-{}.json",
            std::process::id()
        ));
        std::fs::write(&file, r#"{"params":{"rate":7}}"#).expect("write");
        assert_eq!(
            scenario_arg(file.to_str().expect("utf8 path")).unwrap(),
            json!({"params": {"rate": 7}})
        );
        std::fs::remove_file(&file).expect("cleanup");

        assert_eq!(
            scenario_arg("cofc-1498-patent-case").unwrap(),
            json!("cofc-1498-patent-case")
        );
    }

    #[test]
    fn shorthand_defaults_to_every_pack_and_an_empty_scenario() {
        let req = shorthand(&Cli {
            command: "solve".into(),
            ..cli()
        })
        .unwrap();
        assert_eq!(req["packs"], json!([] as [&str; 0]));
        assert_eq!(req["links"], json!(true));
        assert_eq!(req["scenario"], json!({}));
        assert_eq!(req["op"], json!({"op": "solve"}));
    }

    #[test]
    fn shorthand_splits_packs_and_applies_arg_and_no_links() {
        let req = shorthand(&Cli {
            command: "explain".into(),
            packs: Some("cofc,cafc".into()),
            args: vec!["node=answer-due".into(), "top=5".into()],
            no_links: true,
            ..cli()
        })
        .unwrap();
        assert_eq!(req["packs"], json!(["cofc", "cafc"]));
        assert_eq!(req["links"], json!(false));
        assert_eq!(
            req["op"],
            json!({"op": "explain", "node": "answer-due", "top": 5.0})
        );
    }

    #[test]
    fn shorthand_composes_a_bare_named_scenario_with_set_via_extends() {
        let req = shorthand(&Cli {
            command: "chain".into(),
            scenario: Some("cofc-1498-patent-case".into()),
            set: vec!["rate=999".into()],
            ..cli()
        })
        .unwrap();
        assert_eq!(
            req["scenario"],
            json!({"extends": "cofc-1498-patent-case", "params": {"rate": 999.0}})
        );
    }

    #[test]
    fn shorthand_set_on_an_inline_scenario_adds_params_directly() {
        let req = shorthand(&Cli {
            command: "chain".into(),
            scenario: Some(r#"{"payoffs":{"cofc::x":1}}"#.into()),
            set: vec!["rate=100".into()],
            ..cli()
        })
        .unwrap();
        assert_eq!(
            req["scenario"],
            json!({"payoffs": {"cofc::x": 1}, "params": {"rate": 100.0}})
        );
    }

    #[test]
    fn shorthand_rejects_a_non_numeric_set_value() {
        let e = shorthand(&Cli {
            command: "chain".into(),
            set: vec!["rate=fast".into()],
            ..cli()
        })
        .unwrap_err();
        assert!(e.to_string().contains("not a number"), "{e}");
    }

    #[test]
    fn shorthand_rejects_a_stray_positional_argument() {
        let e = shorthand(&Cli {
            command: "chain".into(),
            input: Some("stray.json".into()),
            ..cli()
        })
        .unwrap_err();
        assert!(e.to_string().contains("unexpected argument"), "{e}");
    }

    #[test]
    fn load_catalog_defaults_to_the_embedded_packs_and_scenarios() {
        let c = load_catalog(&cli()).unwrap();
        assert_eq!(c.origin, "embedded");
        assert_eq!(c.scenarios_origin, "embedded");
        assert!(c.scenarios().count() >= 4);
    }

    #[test]
    fn load_catalog_honors_explicit_packs_dir_and_scenarios_dir() {
        let c = load_catalog(&Cli {
            packs_dir: Some(packs_dir()),
            scenarios_dir: Some(scenarios_dir()),
            ..cli()
        })
        .unwrap();
        assert_eq!(c.origin, packs_dir());
        assert_eq!(c.scenarios_origin, scenarios_dir());
    }

    #[test]
    fn load_catalog_reports_a_missing_packs_dir_as_an_error() {
        let e = load_catalog(&Cli {
            packs_dir: Some("/no/such/packs/dir".into()),
            ..cli()
        })
        .unwrap_err();
        assert!(e.to_string().contains("loading packs"), "{e}");
    }

    #[test]
    fn run_schema_defaults_to_request_and_rejects_an_unknown_kind() {
        assert!(run(&Cli {
            command: "schema".into(),
            compact: true,
            ..cli()
        })
        .unwrap());
        assert!(run(&Cli {
            command: "schema".into(),
            input: Some("no-such-kind".into()),
            compact: true,
            ..cli()
        })
        .is_err());
    }

    #[test]
    fn run_validate_reports_valid_and_invalid_documents() {
        assert!(run(&Cli {
            command: "validate".into(),
            input: Some(r#"{"params":{"rate":1}}"#.into()),
            compact: true,
            ..cli()
        })
        .unwrap());
        assert!(!run(&Cli {
            command: "validate".into(),
            input: Some(r#"{"not_a_real_field":1}"#.into()),
            compact: true,
            ..cli()
        })
        .unwrap());
    }

    #[test]
    fn run_q_reports_ok_and_failed_requests() {
        assert!(run(&Cli {
            command: "q".into(),
            input: Some(r#"{"packs":["cofc"],"op":{"op":"solve"}}"#.into()),
            compact: true,
            ..cli()
        })
        .unwrap());
        assert!(!run(&Cli {
            command: "run".into(),
            input: Some(r#"{"scenario":"no-such-scenario","op":{"op":"solve"}}"#.into()),
            compact: true,
            ..cli()
        })
        .unwrap());
    }

    #[test]
    fn run_shorthand_op_with_a_named_scenario_and_set() {
        assert!(run(&Cli {
            command: "chain".into(),
            scenario: Some("cofc-1498-patent-case".into()),
            set: vec!["rate=750".into()],
            compact: true,
            ..cli()
        })
        .unwrap());
    }

    #[test]
    fn emit_prints_compact_and_pretty() {
        emit(&json!({"a": 1}), true).unwrap();
        emit(&json!({"a": 1}), false).unwrap();
    }
}
