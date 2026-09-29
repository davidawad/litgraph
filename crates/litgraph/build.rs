// SPDX-License-Identifier: GPL-3.0-or-later
//! Embed the repository's `packs/` directory into the library so the binary
//! works standalone (no packs directory needed at runtime). `LITGRAPH_PACKS`
//! or `--packs-dir` still override at runtime.

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let packs = manifest.join("../../packs");
    println!("cargo:rerun-if-changed={}", packs.display());
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&packs)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    entries.retain(|p| p.extension().is_some_and(|x| x == "json"));
    entries.sort();
    let mut src = String::from("/// `(file name, contents)` of every embedded pack file (including `links.json`).\npub static EMBEDDED: &[(&str, &str)] = &[\n");
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
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default()).join("embedded_packs.rs");
    if let Err(e) = std::fs::write(&out, src) {
        panic!("writing {}: {e}", out.display());
    }
}
