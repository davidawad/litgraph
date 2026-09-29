// SPDX-License-Identifier: GPL-3.0-or-later
//! `litgraph-mcp` — stdio MCP server binary. Loads packs exactly like the
//! CLI (`$LITGRAPH_PACKS`, `--packs-dir`, else the packs embedded in this
//! binary at build time) and serves [`litgraph_mcp::LitgraphServer`] over
//! stdio until the peer disconnects.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use litgraph::api::Catalog;
use litgraph_mcp::LitgraphServer;
use rmcp::transport::stdio;
use rmcp::ServiceExt;

/// MCP stdio server for litgraph: every engine op as a tool, packs /
/// `links.json` / the manual as resources.
#[derive(Parser)]
#[command(
    name = "litgraph-mcp",
    version,
    about = "MCP stdio server for litgraph: every engine op as a tool, packs/links.json/the manual as resources."
)]
struct Cli {
    /// Packs directory (default: `$LITGRAPH_PACKS`, else the packs built
    /// into this binary).
    #[arg(long)]
    packs_dir: Option<PathBuf>,
}

fn load_catalog(cli: &Cli) -> anyhow::Result<Catalog> {
    match &cli.packs_dir {
        Some(dir) => Catalog::load(dir).map_err(Into::into),
        None => Catalog::default_source().map_err(Into::into),
    }
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

    #[test]
    fn load_catalog_defaults_to_the_embedded_packs() {
        let cli = Cli { packs_dir: None };
        let catalog = load_catalog(&cli).expect("embedded packs must load");
        assert_eq!(catalog.origin, "embedded");
    }

    #[test]
    fn load_catalog_honors_packs_dir() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        assert!(dir.is_dir(), "expected a packs/ dir at {}", dir.display());
        let cli = Cli {
            packs_dir: Some(dir.clone()),
        };
        let catalog = load_catalog(&cli).expect("the repo's packs/ dir must load");
        assert_eq!(catalog.origin, dir.display().to_string());
    }

    #[test]
    fn load_catalog_reports_a_missing_directory_as_an_error() {
        let cli = Cli {
            packs_dir: Some(PathBuf::from("/no/such/litgraph-packs-dir")),
        };
        assert!(load_catalog(&cli).is_err());
    }
}
