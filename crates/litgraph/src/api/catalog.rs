// SPDX-License-Identifier: GPL-3.0-or-later
//! Where packs come from: the set embedded at build time, or a directory.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::{CompileOptions, Graph, LinkFile, Pack};

include!(concat!(env!("OUT_DIR"), "/embedded_packs.rs"));

/// A set of packs plus their `links.json`.
#[derive(Debug, Clone)]
pub struct Catalog {
    /// `embedded` or the directory the packs were read from.
    pub origin: String,
    /// `(file name, pack, content fingerprint)`, sorted by file name.
    pub packs: Vec<(String, Pack, String)>,
    /// Cross-pack links and instances.
    pub links: LinkFile,
}

/// FNV-1a 64 content fingerprint (not cryptographic): enough to tell that a
/// pack changed between two analyses.
#[must_use]
pub fn fingerprint(bytes: &[u8]) -> String {
    let h = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("fnv1a64:{h:016x}")
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
        })
    }

    /// The packs compiled into this build.
    ///
    /// # Errors
    /// Only if an embedded pack is malformed (caught by the test suite).
    pub fn embedded() -> Result<Catalog> {
        Catalog::from_files(
            "embedded".into(),
            EMBEDDED
                .iter()
                .map(|(n, t)| ((*n).to_string(), (*t).to_string())),
        )
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

    /// `$LITGRAPH_PACKS` if set, else the embedded packs.
    ///
    /// # Errors
    /// As [`Catalog::load`] / [`Catalog::embedded`].
    pub fn default_source() -> Result<Catalog> {
        match std::env::var_os("LITGRAPH_PACKS") {
            Some(d) => Catalog::load(&PathBuf::from(d)),
            None => Catalog::embedded(),
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

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(fingerprint(b""), "fnv1a64:cbf29ce484222325");
        assert_ne!(fingerprint(b"a"), fingerprint(b"b"));
    }
}
