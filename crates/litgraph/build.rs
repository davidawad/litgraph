// SPDX-License-Identifier: GPL-3.0-or-later
//! Embed the repository's `packs/`, `scenarios/`, `calibration/` and
//! `sources/` directories into the library so the binary works standalone
//! (no directory needed at runtime). `LITGRAPH_PACKS`/`--packs-dir`,
//! `LITGRAPH_SCENARIOS`/`--scenarios-dir`, `LITGRAPH_CALIBRATION`/
//! `--calibration-dir` and `LITGRAPH_SOURCES` still override at runtime,
//! independently of each other.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());

    embed_dir(
        &manifest.join("../../packs"),
        "json",
        None,
        "EMBEDDED",
        "`(file name, contents)` of every embedded pack file (including `links.json`).",
        &out_dir.join("embedded_packs.rs"),
    );
    embed_dir(
        &manifest.join("../../scenarios"),
        "json",
        None,
        "EMBEDDED_SCENARIOS",
        "`(file name, contents)` of every embedded named-scenario file.",
        &out_dir.join("embedded_scenarios.rs"),
    );
    embed_dir(
        &manifest.join("../../calibration"),
        "json",
        None,
        "EMBEDDED_CALIBRATION",
        "`(file name, contents)` of every embedded calibration set file.",
        &out_dir.join("embedded_calibration.rs"),
    );
    // Without `embed-sources`, embed an empty corpus (point at a directory
    // that has no `*.txt`), keeping the static's shape identical.
    let sources_dir = if std::env::var_os("CARGO_FEATURE_EMBED_SOURCES").is_some() {
        manifest.join("../../sources")
    } else {
        manifest.join("src")
    };
    embed_dir(
        &sources_dir,
        "txt",
        Some("sources/"),
        "SOURCES_EMBEDDED",
        "`(repo-relative path, contents)` of every embedded L0 source text file, matching how `Pack.sources[].path` names them.",
        &out_dir.join("embedded_sources.rs"),
    );
}

/// Embed every `*.<ext>` file directly inside `dir` (non-recursive) as a
/// `pub static <ident>: &[(&str, &str)]`, key optionally prefixed with
/// `key_prefix` (e.g. `"sources/"` so the key matches `Pack.sources[].path`
/// exactly), written to `out_file`.
fn embed_dir(
    dir: &Path,
    ext: &str,
    key_prefix: Option<&str>,
    ident: &str,
    doc: &str,
    out_file: &Path,
) {
    println!("cargo:rerun-if-changed={}", dir.display());
    // A missing directory is a packaging bug (a Dockerfile or nix source
    // filter that forgot to copy it), not an empty library: fail the build
    // instead of silently shipping a binary with nothing embedded.
    let read = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "{} is missing ({e}); every directory build.rs embeds must be in the build \
             context (Dockerfile COPY, flake.nix dataDirs)",
            dir.display()
        )
    });
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.retain(|p| p.extension().is_some_and(|x| x == ext));
    entries.sort();
    let mut src = format!("/// {doc}\npub static {ident}: &[(&str, &str)] = &[\n");
    for p in &entries {
        println!("cargo:rerun-if-changed={}", p.display());
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let key = key_prefix.map_or_else(|| name.to_string(), |prefix| format!("{prefix}{name}"));
        let abs = p.canonicalize().unwrap_or_else(|_| p.clone());
        let _ = writeln!(
            src,
            "    ({key:?}, include_str!({:?})),",
            abs.display().to_string()
        );
    }
    src.push_str("];\n");
    if let Err(e) = std::fs::write(out_file, src) {
        panic!("writing {}: {e}", out_file.display());
    }
}
