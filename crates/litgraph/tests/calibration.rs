// SPDX-License-Identifier: GPL-3.0-or-later
//! End-to-end coverage for the calibration pipeline (`api::calibration`):
//! every embedded `calibration/*.json` entry resolves against the real
//! embedded packs, `scenario.calibration` applies through `api::handle` and
//! is reported in `provenance.calibration`, an unknown set name is a
//! request error, and the `calibration` op surfaces uncalibrated inputs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::{apply_calibration, handle, CalibrationCatalog, Catalog, Request};
use litgraph::model::CompileOptions;
use serde_json::json;

/// Every entry in every embedded calibration set must resolve against the
/// full embedded catalog (all packs + links) — if it doesn't, the ref in the
/// `calibration/*.json` file is wrong (a typo, or the pack it targets was
/// renamed), and this test is what catches it instead of the entry silently
/// no-op'ing at request time.
#[test]
fn every_embedded_calibration_entry_resolves() {
    let packs = Catalog::embedded().expect("embedded packs");
    let calib = CalibrationCatalog::embedded().expect("embedded calibration");
    assert!(
        calib.sets.len() >= 4,
        "expected at least the ptab/cafc/cofc/frcp FY2024 sets, got {}",
        calib.sets.len()
    );
    for (file, set, _) in &calib.sets {
        let mut g = packs
            .compile(&[], true, &CompileOptions::default())
            .expect("compile full embedded graph");
        let app = apply_calibration(&mut g, set);
        assert_eq!(
            app.skipped,
            0,
            "{file} (set {}): {} of {} entries did not resolve against the embedded packs",
            set.id,
            app.skipped,
            set.entries.len()
        );
        assert_eq!(app.applied.len(), set.entries.len());
    }
}

/// `scenario.calibration` applies through the public `handle` entry point
/// and is reported, with source/vintage/n, under `provenance.calibration`.
#[test]
fn scenario_calibration_applies_and_is_reported_in_provenance() {
    let catalog = Catalog::embedded().unwrap();
    let req: Request = serde_json::from_value(json!({
        "packs": ["ptab-patent-trial-appeal-board"],
        "scenario": { "calibration": ["ptab-fy2024"] },
        "op": { "op": "validate" }
    }))
    .unwrap();
    let resp = handle(&req, &catalog);
    assert!(resp.ok, "{:?}", resp.error);
    let prov = resp.provenance.expect("provenance");
    let apps = prov["calibration"].as_array().expect("calibration array");
    assert_eq!(apps.len(), 1);
    let app = &apps[0];
    assert_eq!(app["set"], "ptab-fy2024");
    assert_eq!(app["skipped"], 0);
    let applied = app["applied"].as_array().unwrap();
    assert_eq!(applied.len(), 6, "all 6 ptab-fy2024 entries should resolve");
    let institution = applied
        .iter()
        .find(|a| a["ref"].as_str().unwrap().contains("institution-granted"))
        .expect("institution-granted entry");
    assert!((institution["value"].as_f64().unwrap() - 0.681).abs() < 1e-6);
    assert_eq!(institution["n"], 1087);
    assert!(institution["sourceUrl"]
        .as_str()
        .unwrap()
        .starts_with("https://www.uspto.gov/"));
}

/// A request that doesn't opt into calibration reports an empty list, not a
/// missing field — an agent can always look at `provenance.calibration`
/// without checking whether the key exists first.
#[test]
fn no_calibration_requested_reports_empty_list() {
    let catalog = Catalog::embedded().unwrap();
    let req: Request = serde_json::from_value(json!({
        "packs": ["cofc"],
        "op": { "op": "describe" }
    }))
    .unwrap();
    let resp = handle(&req, &catalog);
    assert!(resp.ok);
    let prov = resp.provenance.unwrap();
    assert_eq!(prov["calibration"], json!([]));
}

/// An unknown calibration set name is a normal `ok: false` request error
/// (`not-found`), not a panic.
#[test]
fn unknown_calibration_set_is_a_request_error() {
    let catalog = Catalog::embedded().unwrap();
    let req: Request = serde_json::from_value(json!({
        "packs": ["cofc"],
        "scenario": { "calibration": ["no-such-set"] },
        "op": { "op": "describe" }
    }))
    .unwrap();
    let resp = handle(&req, &catalog);
    assert!(!resp.ok);
    let err = resp.error.unwrap();
    assert_eq!(err.code, "not-found");
    assert!(err.message.contains("no-such-set"));
}

/// The `calibration` op ranks uncalibrated probabilities/durations by
/// decision sensitivity; the FRCP pack (no calibration applied here) has
/// plenty of both.
#[test]
fn calibration_op_lists_gaps_ranked_by_swing() {
    let catalog = Catalog::embedded().unwrap();
    let req: Request = serde_json::from_value(json!({
        "packs": ["frcp-civil-procedure"],
        "op": { "op": "calibration", "top": 5, "max_candidates": 40 }
    }))
    .unwrap();
    let resp = handle(&req, &catalog);
    assert!(resp.ok, "{:?}", resp.error);
    let result = resp.result.unwrap();
    assert!(result["base_value"].is_number());
    assert!(result["base_elapsed_days"].is_number());
    let durations = result["durations"].as_array().unwrap();
    assert!(!durations.is_empty(), "frcp has uncalibrated durations");
    assert!(durations.len() <= 5);
    // Sorted descending by swing.
    let swings: Vec<f64> = durations
        .iter()
        .map(|r| r["swing"].as_f64().unwrap())
        .collect();
    assert!(swings.windows(2).all(|w| w[0] >= w[1]), "{swings:?}");
}

/// Applying `ptab-fy2024` calibration is visible on the graph itself: the
/// resulting authored probability matches the calibration value, and the
/// three FWD-outcome edges (which the calibration set makes sum to exactly
/// 1.0) trigger no `probability-renormalized` warning.
#[test]
fn calibrated_fwd_outcomes_need_no_renormalization() {
    let catalog = Catalog::embedded().unwrap();
    let req: Request = serde_json::from_value(json!({
        "packs": ["ptab-patent-trial-appeal-board"],
        "scenario": { "calibration": ["ptab-fy2024"] },
        "op": { "op": "graph", "node": "fwd-issued" }
    }))
    .unwrap();
    let resp = handle(&req, &catalog);
    assert!(resp.ok, "{:?}", resp.error);
    let renorm = resp.warnings.iter().any(|w| {
        w.code == "probability-renormalized" && w.at.iter().any(|a| a.contains("fwd-issued"))
    });
    assert!(
        !renorm,
        "calibrated FWD outcomes already sum to 1.0: {:?}",
        resp.warnings
    );
}
