// SPDX-License-Identifier: GPL-3.0-or-later
//! Embed the repository's `packs/`, `scenarios/` and `calibration/`
//! directories into the library so the binary works standalone (no
//! directory needed at runtime). `LITGRAPH_PACKS`/`--packs-dir`,
//! `LITGRAPH_SCENARIOS`/`--scenarios-dir` and
//! `LITGRAPH_CALIBRATION`/`--calibration-dir` still override at runtime,
//! independently of each other.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Embeds every `*.json` file directly under `dir` as a
/// `pub static <static_name>: &[(&str, &str)]` in `<out_name>`, so it can be
/// pulled in with `include!(concat!(env!("OUT_DIR"), "/<out_name>"))`.
fn embed_dir(dir: &Path, static_name: &str, out_name: &str, doc: &str) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    entries.retain(|p| p.extension().is_some_and(|x| x == "json"));
    entries.sort();
    let mut src = format!("/// {doc}\npub static {static_name}: &[(&str, &str)] = &[\n");
    for p in &entries {
        println!("cargo:rerun-if-changed={}", p.display());
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let abs = p.canonicalize().unwrap_or_else(|_| p.clone());
        let _ = writeln!(
            src,
            "    ({name:?}, include_str!({:?})),",
            abs.display().to_string()
        );
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default()).join(out_name);
    if let Err(e) = std::fs::write(&out, src) {
        panic!("writing {}: {e}", out.display());
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    embed_dir(
        &manifest.join("../../packs"),
        "EMBEDDED",
        "embedded_packs.rs",
        "`(file name, contents)` of every embedded pack file (including `links.json`).",
    );
    embed_dir(
        &manifest.join("../../scenarios"),
        "EMBEDDED_SCENARIOS",
        "embedded_scenarios.rs",
        "`(file name, contents)` of every embedded named-scenario file.",
    );
    embed_dir(
        &manifest.join("../../calibration"),
        "EMBEDDED_CALIBRATION",
        "embedded_calibration.rs",
        "`(file name, contents)` of every embedded calibration set file.",
    );
}
