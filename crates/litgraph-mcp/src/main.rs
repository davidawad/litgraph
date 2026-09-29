// SPDX-License-Identifier: GPL-3.0-or-later
//! `litgraph-mcp` — stdio MCP server binary. Loads packs and the named
//! scenario library exactly like the CLI (`$LITGRAPH_PACKS`/`--packs-dir`,
//! `$LITGRAPH_SCENARIOS`/`--scenarios-dir`, else what's embedded in this
//! binary at build time) and serves [`litgraph_mcp::LitgraphServer`] over
//! stdio until the peer disconnects.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;
use litgraph::api::Catalog;
use litgraph_mcp::LitgraphServer;
use rmcp::transport::stdio;
use rmcp::ServiceExt;

/// MCP stdio server for litgraph: every engine op as a tool, packs /
/// `links.json` / the scenario library / the manual as resources.
#[derive(Parser)]
#[command(
    name = "litgraph-mcp",
    version,
    about = "MCP stdio server for litgraph: every engine op as a tool, packs/links.json/scenarios/the manual as resources."
)]
struct Cli {
    /// Packs directory (default: `$LITGRAPH_PACKS`, else the packs built
    /// into this binary).
    #[arg(long)]
    packs_dir: Option<PathBuf>,
    /// Scenario library directory (default: `$LITGRAPH_SCENARIOS`, else the
    /// scenarios built into this binary).
    #[arg(long)]
    scenarios_dir: Option<PathBuf>,
}

/// Resolve packs and the scenario library independently: `--packs-dir` /
/// `--scenarios-dir` each fall back to their own env var, else what's built
/// into this binary -- the same precedent as `litgraph-cli`'s own
/// `load_catalog`.
fn load_catalog(cli: &Cli) -> anyhow::Result<Catalog> {
    let catalog = match &cli.packs_dir {
        Some(dir) => Catalog::load(dir),
        None => Catalog::default_source(),
    }
    .context("loading packs")?;
    let scenarios_dir = cli
        .scenarios_dir
        .clone()
        .or_else(|| std::env::var_os("LITGRAPH_SCENARIOS").map(PathBuf::from));
    match scenarios_dir {
        Some(dir) => catalog.with_scenarios_dir(&dir),
        None => catalog.with_embedded_scenarios(),
    }
    .context("loading scenarios")
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let catalog = match load_catalog(&cli) {
        Ok(catalog) => catalog,
        Err(e) => {
            eprintln!("litgraph-mcp: loading packs: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    let server = LitgraphServer::new(catalog);
    let service = match server.serve(stdio()).await {
        Ok(service) => service,
        Err(e) => {
            eprintln!("litgraph-mcp: starting server: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    match service.waiting().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("litgraph-mcp: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli() -> Cli {
        Cli {
            packs_dir: None,
            scenarios_dir: None,
        }
    }

    fn repo_dir(name: &str) -> PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../{name}"));
        assert!(dir.is_dir(), "expected a {name}/ dir at {}", dir.display());
        dir
    }

    #[test]
    fn load_catalog_defaults_to_the_embedded_packs_and_scenarios() {
        let catalog = load_catalog(&cli()).expect("embedded packs/scenarios must load");
        assert_eq!(catalog.origin, "embedded");
        assert_eq!(catalog.scenarios_origin, "embedded");
        assert!(!catalog.scenarios.is_empty());
    }

    #[test]
    fn load_catalog_honors_packs_dir() {
        let dir = repo_dir("packs");
        let catalog = load_catalog(&Cli {
            packs_dir: Some(dir.clone()),
            ..cli()
        })
        .expect("the repo's packs/ dir must load");
        assert_eq!(catalog.origin, dir.display().to_string());
    }

    #[test]
    fn load_catalog_reports_a_missing_packs_directory_as_an_error() {
        let bad = Cli {
            packs_dir: Some(PathBuf::from("/no/such/litgraph-packs-dir")),
            ..cli()
        };
        assert!(load_catalog(&bad).is_err());
    }

    #[test]
    fn load_catalog_honors_scenarios_dir_independently_of_packs_dir() {
        let dir = repo_dir("scenarios");
        let catalog = load_catalog(&Cli {
            scenarios_dir: Some(dir.clone()),
            ..cli()
        })
        .expect("the repo's scenarios/ dir must load");
        // Packs still resolve to the embedded set -- overriding one half
        // never disturbs the other.
        assert_eq!(catalog.origin, "embedded");
        assert_eq!(catalog.scenarios_origin, dir.display().to_string());
        assert!(!catalog.scenarios.is_empty());
    }

    #[test]
    fn load_catalog_reports_a_missing_scenarios_directory_as_an_error() {
        let bad = Cli {
            scenarios_dir: Some(PathBuf::from("/no/such/litgraph-scenarios-dir")),
            ..cli()
        };
        assert!(load_catalog(&bad).is_err());
    }
}
