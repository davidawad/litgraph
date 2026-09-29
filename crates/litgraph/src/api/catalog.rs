// SPDX-License-Identifier: GPL-3.0-or-later
//! Where packs come from: the set embedded at build time, or a directory.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::{CompileOptions, Graph, LinkFile, Pack};
use crate::scenario::NamedScenario;

include!(concat!(env!("OUT_DIR"), "/embedded_packs.rs"));
include!(concat!(env!("OUT_DIR"), "/embedded_scenarios.rs"));

/// A set of packs plus their `links.json`, and a named-scenario library.
#[derive(Debug, Clone)]
pub struct Catalog {
    /// `embedded` or the directory the packs were read from.
    pub origin: String,
    /// `(file name, pack, content fingerprint)`, sorted by file name.
    pub packs: Vec<(String, Pack, String)>,
    /// Cross-pack links and instances.
    pub links: LinkFile,
    /// `embedded` or the directory the scenario library was read from.
    pub scenarios_origin: String,
    /// `(file name, scenario, content fingerprint)`, sorted by file name.
    pub scenarios: Vec<(String, NamedScenario, String)>,
}

/// FNV-1a 64 content fingerprint (not cryptographic): enough to tell that a
/// pack changed between two analyses.
#[must_use]
pub fn fingerprint(bytes: &[u8]) -> String {
    format!("fnv1a64:{:016x}", fnv1a64(bytes))
}

/// FNV-1a, 64-bit.
#[must_use]
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl Catalog {
    /// Build a catalog from `(file name, JSON text)` pairs; `links.json` is the link file.
    ///
    /// # Errors
    /// Malformed packs or links.
    pub fn from_files(
        origin: String,
        files: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Catalog> {
        let mut packs = vec![];
        let mut links = LinkFile::default();
        for (name, text) in files {
            if name == "links.json" {
                links = serde_json::from_str(&text)
                    .map_err(|e| Error::Parse(format!("links.json: {e}")))?;
            } else {
                let pack =
                    Pack::from_json(&text).map_err(|e| Error::Parse(format!("{name}: {e}")))?;
                packs.push((name, pack, fingerprint(text.as_bytes())));
            }
        }
        packs.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Catalog {
            origin,
            packs,
            links,
            scenarios_origin: String::new(),
            scenarios: vec![],
        })
    }

    /// The packs compiled into this build, with the embedded scenario
    /// library attached.
    ///
    /// # Errors
    /// Only if an embedded pack or scenario is malformed (caught by the test
    /// suite).
    pub fn embedded() -> Result<Catalog> {
        Catalog::from_files(
            "embedded".into(),
            EMBEDDED
                .iter()
                .map(|(n, t)| ((*n).to_string(), (*t).to_string())),
        )?
        .with_embedded_scenarios()
    }

    /// Attach the scenario library embedded in this binary, replacing
    /// whatever scenarios (if any) this catalog already had.
    ///
    /// # Errors
    /// Only if an embedded scenario is malformed (caught by the test suite).
    pub fn with_embedded_scenarios(self) -> Result<Catalog> {
        self.with_scenario_files(
            "embedded".into(),
            EMBEDDED_SCENARIOS
                .iter()
                .map(|(n, t)| ((*n).to_string(), (*t).to_string())),
        )
    }

    /// Attach every `*.json` scenario file in `dir`, replacing whatever
    /// scenarios (if any) this catalog already had.
    ///
    /// # Errors
    /// Unreadable directory or a malformed scenario file.
    pub fn with_scenarios_dir(self, dir: &Path) -> Result<Catalog> {
        let read =
            std::fs::read_dir(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        let mut files = vec![];
        for entry in read.filter_map(std::result::Result::ok) {
            let p = entry.path();
            if p.extension().is_some_and(|x| x == "json") {
                let text = std::fs::read_to_string(&p)
                    .map_err(|e| Error::Io(format!("{}: {e}", p.display())))?;
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                files.push((name, text));
            }
        }
        self.with_scenario_files(dir.display().to_string(), files)
    }

    fn with_scenario_files(
        mut self,
        origin: String,
        files: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Catalog> {
        let mut scenarios = vec![];
        for (name, text) in files {
            let def = NamedScenario::from_json(&text)
                .map_err(|e| Error::Parse(format!("{name}: {e}")))?;
            if let Some((dup, ..)) = scenarios
                .iter()
                .find(|(_, d, _): &&(String, NamedScenario, String)| d.id == def.id)
            {
                return Err(Error::Parse(format!(
                    "scenario {name}: id `{}` duplicates {dup}",
                    def.id
                )));
            }
            scenarios.push((name, def, fingerprint(text.as_bytes())));
        }
        scenarios.sort_by(|a, b| a.0.cmp(&b.0));
        self.scenarios_origin = origin;
        self.scenarios = scenarios;
        Ok(self)
    }

    /// Every scenario in the library.
    pub fn scenarios(&self) -> impl Iterator<Item = &NamedScenario> {
        self.scenarios.iter().map(|(_, s, _)| s)
    }

    /// Resolve a scenario ref: its `id`, or the file stem it was loaded from.
    ///
    /// # Errors
    /// `NotFound` listing the available names.
    pub fn scenario(&self, name: &str) -> Result<&NamedScenario> {
        self.scenarios
            .iter()
            .find(|(file, s, _)| s.id == name || file.strip_suffix(".json") == Some(name))
            .map(|(_, s, _)| s)
            .ok_or_else(|| {
                let ids: Vec<&str> = self
                    .scenarios
                    .iter()
                    .map(|(_, s, _)| s.id.as_str())
                    .collect();
                Error::NotFound(format!("scenario {name}; available: {}", ids.join(", ")))
            })
    }

    /// Every `*.json` in `dir` (and `links.json` if present).
    ///
    /// # Errors
    /// Unreadable directory or malformed pack.
    pub fn load(dir: &Path) -> Result<Catalog> {
        let read =
            std::fs::read_dir(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        let mut files = vec![];
        for entry in read.filter_map(std::result::Result::ok) {
            let p = entry.path();
            if p.extension().is_some_and(|x| x == "json") {
                let text = std::fs::read_to_string(&p)
                    .map_err(|e| Error::Io(format!("{}: {e}", p.display())))?;
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                files.push((name, text));
            }
        }
        Catalog::from_files(dir.display().to_string(), files)
    }

    /// `$LITGRAPH_PACKS` if set, else the embedded packs; independently,
    /// `$LITGRAPH_SCENARIOS` if set, else the embedded scenario library.
    ///
    /// # Errors
    /// As [`Catalog::load`] / [`Catalog::embedded`] / [`Catalog::with_scenarios_dir`].
    pub fn default_source() -> Result<Catalog> {
        let catalog = match std::env::var_os("LITGRAPH_PACKS") {
            Some(d) => Catalog::load(&PathBuf::from(d))?,
            None => Catalog::from_files(
                "embedded".into(),
                EMBEDDED
                    .iter()
                    .map(|(n, t)| ((*n).to_string(), (*t).to_string())),
            )?,
        };
        match std::env::var_os("LITGRAPH_SCENARIOS") {
            Some(d) => catalog.with_scenarios_dir(&PathBuf::from(d)),
            None => catalog.with_embedded_scenarios(),
        }
    }

    /// Resolve pack refs: id, forum key, file stem, or a path to a pack file.
    /// Empty (or `all`) selects every pack.
    ///
    /// # Errors
    /// `NotFound` listing the available ids.
    pub fn select(&self, refs: &[String]) -> Result<Vec<(Pack, String)>> {
        if refs.is_empty() || refs.iter().any(|r| r == "all") {
            return Ok(self
                .packs
                .iter()
                .map(|(_, p, h)| (p.clone(), h.clone()))
                .collect());
        }
        refs.iter().map(|r| self.select_one(r)).collect()
    }

    fn select_one(&self, r: &str) -> Result<(Pack, String)> {
        if Path::new(r).extension().is_some_and(|x| x == "json") && Path::new(r).exists() {
            let text = std::fs::read_to_string(r).map_err(|e| Error::Io(e.to_string()))?;
            return Ok((Pack::from_json(&text)?, fingerprint(text.as_bytes())));
        }
        self.packs
            .iter()
            .find(|(file, p, _)| {
                p.id == r || p.forum.as_deref() == Some(r) || file.strip_suffix(".json") == Some(r)
            })
            .map(|(_, p, h)| (p.clone(), h.clone()))
            .ok_or_else(|| {
                let ids: Vec<&str> = self.packs.iter().map(|(_, p, _)| p.id.as_str()).collect();
                Error::NotFound(format!("pack {r}; available: {}", ids.join(", ")))
            })
    }

    /// Compile the referenced packs (with links unless `links` is false).
    ///
    /// # Errors
    /// Selection or compilation errors.
    pub fn compile(&self, refs: &[String], links: bool, opts: &CompileOptions) -> Result<Graph> {
        let packs: Vec<Pack> = self.select(refs)?.into_iter().map(|(p, _)| p).collect();
        let lf = if links {
            self.links.clone()
        } else {
            LinkFile::default()
        };
        Graph::compile(&packs, &lf, opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_loads_and_compiles_everything() -> Result<()> {
        let c = Catalog::embedded()?;
        assert!(c.packs.len() >= 8, "embedded packs: {}", c.packs.len());
        assert!(!c.links.links.is_empty());
        let g = c.compile(&[], true, &CompileOptions::default())?;
        assert!(g.nodes.len() > 400);
        assert!(c.select(&["nope".into()]).is_err());
        assert_eq!(c.select(&["cofc".into()])?.len(), 1);
        Ok(())
    }

    /// Every embedded pack file, checked *individually* against the strict
    /// `Pack` struct, with a per-file assertion message naming exactly which
    /// one failed and why.
    ///
    /// A prior incident: two agents each added a top-level `"sources"` key
    /// to the same pack, at different enough line positions that git's
    /// line-based merge combined them with no conflict marker. The result
    /// was syntactically valid JSON (a generic parser or `serde_json::Value`
    /// tolerates a duplicate object key, keeping the last one silently), but
    /// serde's derived `Deserialize` for the `Pack` struct rejects a
    /// duplicate field -- so every embedded pack failed to compile at
    /// startup, and because dozens of unrelated test files each
    /// independently call `Catalog::embedded()`/`Pack::from_json`, the
    /// *entire* suite failed at once with one root cause smeared across
    /// hundreds of lines of near-identical panics, not a single clear
    /// signal naming the file.
    ///
    /// This test is the single, first, clearly-named place that class of
    /// failure surfaces: it parses each embedded pack file on its own, in a
    /// loop with an assertion message giving the exact file name and parse
    /// error, so a regression reads as one focused failure instead of a
    /// suite-wide cascade.
    #[test]
    fn every_embedded_pack_file_parses_individually_against_the_strict_pack_struct() {
        let mut checked = 0;
        for (name, text) in EMBEDDED {
            if *name == "links.json" {
                continue;
            }
            if let Err(e) = Pack::from_json(text) {
                panic!("embedded pack {name} failed to parse against the Pack struct: {e}");
            }
            checked += 1;
        }
        assert!(
            checked >= 8,
            "expected at least 8 embedded packs, checked {checked}"
        );
    }

    #[test]
    fn embedded_catalog_loads_the_scenario_library() -> Result<()> {
        let c = Catalog::embedded()?;
        assert_eq!(c.scenarios_origin, "embedded");
        assert!(
            c.scenarios().count() >= 4,
            "embedded scenarios: {}",
            c.scenarios().count()
        );
        assert!(c.scenario("nope").is_err());
        Ok(())
    }

    /// `with_scenarios_dir` reads every `*.json` in a directory and reports a
    /// clear error naming the unknown id, matching pack selection.
    #[test]
    fn with_scenarios_dir_reads_a_directory_and_reports_unknown_names() -> Result<()> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios");
        assert!(
            dir.is_dir(),
            "expected a scenarios/ dir at {}",
            dir.display()
        );
        let c = Catalog::embedded()?.with_scenarios_dir(&dir)?;
        assert_eq!(c.scenarios_origin, dir.display().to_string());
        assert!(!c.scenarios().collect::<Vec<_>>().is_empty());
        let err = c.scenario("definitely-not-a-scenario").unwrap_err();
        assert!(err.to_string().contains("available:"), "{err}");
        Ok(())
    }

    /// `default_source` reads `$LITGRAPH_SCENARIOS` independently of
    /// `$LITGRAPH_PACKS` (nextest gives every test its own process).
    #[test]
    fn default_source_honors_litgraph_scenarios_env_var() -> Result<()> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios");
        std::env::set_var("LITGRAPH_SCENARIOS", &dir);
        let c = Catalog::default_source()?;
        std::env::remove_var("LITGRAPH_SCENARIOS");
        assert_eq!(c.scenarios_origin, dir.display().to_string());
        assert_eq!(c.origin, "embedded");
        Ok(())
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(fingerprint(b""), "fnv1a64:cbf29ce484222325");
        assert_ne!(fingerprint(b"a"), fingerprint(b"b"));
    }

    /// `default_source` reads `$LITGRAPH_PACKS` when set (nextest gives every
    /// test its own process, so mutating the env var here is isolated).
    #[test]
    fn default_source_honors_litgraph_packs_env_var() -> Result<()> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        assert!(dir.is_dir(), "expected a packs/ dir at {}", dir.display());
        std::env::set_var("LITGRAPH_PACKS", &dir);
        let c = Catalog::default_source()?;
        std::env::remove_var("LITGRAPH_PACKS");
        assert_eq!(c.origin, dir.display().to_string());
        assert!(!c.packs.is_empty());
        Ok(())
    }

    /// `compile(.., links=false, ..)` drops the link file entirely: a graph
    /// compiled without links has strictly fewer (or equal) nodes than one
    /// compiled with them, since cross-forum link edges add `#end` twins.
    #[test]
    fn compile_without_links_uses_an_empty_link_file() -> Result<()> {
        let c = Catalog::embedded()?;
        let with_links = c.compile(&[], true, &CompileOptions::default())?;
        let without_links = c.compile(&[], false, &CompileOptions::default())?;
        assert!(without_links.nodes.len() <= with_links.nodes.len());
        assert!(with_links.nodes.len() > without_links.nodes.len());
        Ok(())
    }
}
