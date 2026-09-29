// SPDX-License-Identifier: GPL-3.0-or-later
//! API-layer coverage for the `deadlines` op (`api::deadlines`): edge/node/
//! reachable modes, options (service method, additional holidays, clerk
//! inaccessibility), the `forum`-then-id rule-set fallback (including a
//! cross-pack link edge, keyed by its origin node's pack), and every error/
//! warning path (no deadline, unresolvable rule set, non-integer length,
//! bad trigger). Small custom packs, not the shipped forums, so every
//! expected value is hand-traceable.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::{handle, Catalog, Op, Request};
use litgraph::model::Pack;
use serde_json::json;

fn pack(json: serde_json::Value) -> (String, String) {
    let id = json["id"].as_str().unwrap().to_string();
    let p: Pack = serde_json::from_value(json).expect("pack parses");
    (format!("{id}.json"), serde_json::to_string(&p).unwrap())
}

/// `frcp-test`: id contains "frcp" so the rule set resolves via the id
/// fallback (no `forum` field authored). Exercises calendar + court units,
/// a no-deadline edge, and a non-integer-length edge.
fn frcp_pack() -> (String, String) {
    pack(json!({
        "schemaVersion": 2, "id": "frcp-test", "title": "FRCP test", "startNodeId": "trigger",
        "nodes": [
            {"id": "trigger", "kind": "state", "label": "Trigger"},
            {"id": "due", "kind": "state", "label": "Due"},
            {"id": "no-deadline-target", "kind": "state", "label": "No deadline"},
            {"id": "bad-length-target", "kind": "state", "label": "Bad length"}
        ],
        "edges": [
            {"id": "respond", "from": "trigger", "to": "due", "label": "Respond",
             "authority": "FRCP 12(a)(1)(A)(i)", "deadline": {"length": 21}},
            {"id": "court-respond", "from": "trigger", "to": "due", "label": "Respond (court days)",
             "deadline": {"length": 3, "unit": "court"}},
            {"id": "no-deadline", "from": "trigger", "to": "no-deadline-target", "label": "No deadline edge"},
            {"id": "bad-length", "from": "trigger", "to": "bad-length-target", "label": "Bad length",
             "deadline": {"length": 2.5}}
        ]
    }))
}

/// `mystery-forum`: no `forum` field and an id that matches no known forum
/// substring, so the rule set is unresolvable.
fn mystery_pack() -> (String, String) {
    pack(json!({
        "schemaVersion": 2, "id": "mystery-forum", "title": "Mystery", "startNodeId": "trigger",
        "nodes": [
            {"id": "trigger", "kind": "state", "label": "Trigger"},
            {"id": "due", "kind": "state", "label": "Due"}
        ],
        "edges": [
            {"id": "respond", "from": "trigger", "to": "due", "label": "Respond",
             "deadline": {"length": 10}}
        ]
    }))
}

/// `custom-id-cofc`: id matches nothing, but an explicit `forum: "cofc"`
/// should still resolve to `Rcfc6` (forum takes priority over id).
fn explicit_forum_pack() -> (String, String) {
    pack(json!({
        "schemaVersion": 2, "id": "custom-id-cofc", "forum": "cofc",
        "title": "Explicit forum", "startNodeId": "trigger",
        "nodes": [
            {"id": "trigger", "kind": "state", "label": "Trigger"},
            {"id": "due", "kind": "state", "label": "Due"}
        ],
        "edges": [
            {"id": "answer", "from": "trigger", "to": "due", "label": "Answer",
             "deadline": {"length": 60}}
        ]
    }))
}

/// Two packs joined by a `links.json` link edge that itself carries a
/// `deadline`, to exercise the cross-pack-link branch of
/// `ruleset_for_edge` (keyed by the *origin* node's pack — here `frcp-link-origin`).
fn link_origin_pack() -> (String, String) {
    pack(json!({
        "schemaVersion": 2, "id": "frcp-link-origin", "title": "Origin", "startNodeId": "leaves",
        "nodes": [{"id": "leaves", "kind": "state", "label": "Leaves"}],
        "edges": []
    }))
}

fn link_dest_pack() -> (String, String) {
    pack(json!({
        "schemaVersion": 2, "id": "cofc-link-dest", "forum": "cofc", "title": "Dest", "startNodeId": "arrives",
        "nodes": [{"id": "arrives", "kind": "state", "label": "Arrives"}],
        "edges": []
    }))
}

fn links_file() -> (String, String) {
    (
        "links.json".into(),
        json!({
            "links": [{
                "id": "appeal-link", "from": "frcp-link-origin::leaves", "to": "cofc-link-dest::arrives",
                "label": "Appeal", "actor": "applicant", "deadline": {"length": 30}
            }]
        })
        .to_string(),
    )
}

fn catalog(files: Vec<(String, String)>) -> Catalog {
    Catalog::from_files("t".into(), files).expect("catalog compiles")
}

fn deadlines_req(packs: &[&str], op: Op) -> Request {
    Request {
        packs: packs.iter().map(|s| (*s).to_string()).collect(),
        // `Request`'s `#[derive(Default)]` gives `links: false` (bool's own
        // default) — only serde's `#[serde(default = "yes")]` makes JSON
        // requests default to `true`. Set it explicitly so a
        // struct-literal `Request` here matches the JSON API's behavior.
        links: true,
        op,
        ..Request::default()
    }
}

/// A small builder over `Op::Deadlines`'s fields — enum struct-variant
/// functional-record-update (`..base`) isn't stable, so this stands in for
/// it.
struct D {
    trigger: String,
    edge: Option<String>,
    node: Option<String>,
    reachable: bool,
    service_method: Option<litgraph::clock::ServiceMethod>,
    additional_holidays: Vec<String>,
    clerk_inaccessible: bool,
}

impl D {
    fn new(trigger: &str) -> Self {
        D {
            trigger: trigger.into(),
            edge: None,
            node: None,
            reachable: false,
            service_method: None,
            additional_holidays: vec![],
            clerk_inaccessible: false,
        }
    }
    fn edge(mut self, e: &str) -> Self {
        self.edge = Some(e.into());
        self
    }
    fn node(mut self, n: &str) -> Self {
        self.node = Some(n.into());
        self
    }
    fn reachable(mut self) -> Self {
        self.reachable = true;
        self
    }
    fn service(mut self, m: litgraph::clock::ServiceMethod) -> Self {
        self.service_method = Some(m);
        self
    }
    fn clerk(mut self) -> Self {
        self.clerk_inaccessible = true;
        self
    }
    fn holiday(mut self, h: &str) -> Self {
        self.additional_holidays.push(h.into());
        self
    }
    fn op(self) -> Op {
        Op::Deadlines {
            trigger: self.trigger,
            edge: self.edge,
            node: self.node,
            reachable: self.reachable,
            service_method: self.service_method,
            additional_holidays: self.additional_holidays,
            clerk_inaccessible: self.clerk_inaccessible,
        }
    }
}

#[test]
fn edge_mode_computes_one_deadline_with_steps() {
    let c = catalog(vec![frcp_pack()]);
    let resp = handle(
        &deadlines_req(&["frcp-test"], D::new("2026-01-05").edge("respond").op()),
        &c,
    );
    assert!(resp.ok, "{resp:?}");
    let r = resp.result.unwrap();
    assert_eq!(r["count"], json!(1));
    let d = &r["deadlines"][0];
    assert_eq!(d["rule_set"], json!("frcp6"));
    assert_eq!(d["due_date"], json!("2026-01-26"));
    assert!(!d["steps"].as_array().unwrap().is_empty());
    assert_eq!(d["edge"]["id"], json!("frcp-test::respond"));
}

#[test]
fn edge_mode_errors_when_the_edge_has_no_deadline() {
    let c = catalog(vec![frcp_pack()]);
    let resp = handle(
        &deadlines_req(
            &["frcp-test"],
            D::new("2026-01-05").edge("no-deadline").op(),
        ),
        &c,
    );
    assert!(!resp.ok);
    assert_eq!(resp.error.unwrap().code, "invalid");
}

#[test]
fn edge_mode_errors_on_unresolvable_rule_set() {
    let c = catalog(vec![mystery_pack()]);
    let resp = handle(
        &deadlines_req(
            &["mystery-forum"],
            D::new("2026-01-05").edge("respond").op(),
        ),
        &c,
    );
    assert!(!resp.ok);
    assert!(resp.error.unwrap().message.contains("no known"));
}

#[test]
fn node_mode_skips_no_deadline_edges_and_reports_bad_length_as_a_warning() {
    let c = catalog(vec![frcp_pack()]);
    let resp = handle(
        &deadlines_req(&["frcp-test"], D::new("2026-01-05").node("trigger").op()),
        &c,
    );
    assert!(resp.ok, "{resp:?}");
    let r = resp.result.unwrap();
    // Two computable deadlines (calendar + court); "no-deadline" is skipped
    // outright and "bad-length" surfaces as a warning, not a result row.
    assert_eq!(r["count"], json!(2));
    assert!(resp
        .warnings
        .iter()
        .any(|w| w.code == "deadline-uncomputable"));
}

#[test]
fn node_mode_unresolvable_rule_set_is_a_warning_not_an_error() {
    let c = catalog(vec![mystery_pack()]);
    let resp = handle(
        &deadlines_req(
            &["mystery-forum"],
            D::new("2026-01-05").node("trigger").op(),
        ),
        &c,
    );
    assert!(resp.ok, "{resp:?}");
    assert_eq!(resp.result.unwrap()["count"], json!(0));
    assert!(resp
        .warnings
        .iter()
        .any(|w| w.code == "deadline-uncomputable"));
}

#[test]
fn court_days_unit_is_computed_with_business_day_steps() {
    let c = catalog(vec![frcp_pack()]);
    let resp = handle(
        &deadlines_req(
            &["frcp-test"],
            D::new("2026-01-05").edge("court-respond").op(),
        ),
        &c,
    );
    assert!(resp.ok, "{resp:?}");
    let d = &resp.result.unwrap()["deadlines"][0];
    assert_eq!(d["unit"], json!("court"));
    assert!(d["steps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s.as_str().unwrap().contains("court days")));
}

#[test]
fn explicit_forum_field_takes_priority_over_pack_id() {
    let c = catalog(vec![explicit_forum_pack()]);
    let resp = handle(
        &deadlines_req(
            &["custom-id-cofc"],
            D::new("2026-01-05").edge("answer").op(),
        ),
        &c,
    );
    assert!(resp.ok, "{resp:?}");
    assert_eq!(
        resp.result.unwrap()["deadlines"][0]["rule_set"],
        json!("rcfc6")
    );
}

#[test]
fn reachable_mode_walks_past_the_immediate_node() {
    let c = catalog(vec![frcp_pack()]);
    let via_node = handle(
        &deadlines_req(&["frcp-test"], D::new("2026-01-05").node("due").op()),
        &c,
    );
    assert_eq!(via_node.result.unwrap()["count"], json!(0)); // "due" has no out-edges
    let via_reachable = handle(
        &deadlines_req(
            &["frcp-test"],
            D::new("2026-01-05").node("trigger").reachable().op(),
        ),
        &c,
    );
    assert!(via_reachable.ok, "{via_reachable:?}");
    assert_eq!(via_reachable.result.unwrap()["count"], json!(2));
}

#[test]
fn default_node_is_the_pack_start() {
    let c = catalog(vec![frcp_pack()]);
    let resp = handle(
        &deadlines_req(&["frcp-test"], D::new("2026-01-05").op()),
        &c,
    );
    assert!(resp.ok, "{resp:?}");
    assert_eq!(resp.result.unwrap()["node"], json!("frcp-test::trigger"));
}

#[test]
fn service_method_and_clerk_inaccessible_options_are_threaded_through() {
    let c = catalog(vec![frcp_pack()]);
    let resp = handle(
        &deadlines_req(
            &["frcp-test"],
            D::new("2026-01-05")
                .edge("respond")
                .service(litgraph::clock::ServiceMethod::Mail)
                .clerk()
                .op(),
        ),
        &c,
    );
    assert!(resp.ok, "{resp:?}");
    let d = &resp.result.unwrap()["deadlines"][0];
    assert!(d["service_days_added"].as_bool().unwrap());
    assert!(d["clerk_inaccessibility_applied"].as_bool().unwrap());
}

#[test]
fn additional_holidays_option_extends_the_computed_calendar() {
    let c = catalog(vec![frcp_pack()]);
    // Trigger far enough out that "respond" (21 days) would ordinarily land
    // on a plain weekday; declare that exact date a state holiday instead.
    let without = handle(
        &deadlines_req(&["frcp-test"], D::new("2026-01-05").edge("respond").op()),
        &c,
    );
    let due = without.result.unwrap()["deadlines"][0]["due_date"]
        .as_str()
        .unwrap()
        .to_string();
    let with_holiday = handle(
        &deadlines_req(
            &["frcp-test"],
            D::new("2026-01-05").edge("respond").holiday(&due).op(),
        ),
        &c,
    );
    assert!(with_holiday.ok, "{with_holiday:?}");
    let rolled = with_holiday.result.unwrap()["deadlines"][0]["due_date"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(rolled, due);
}

#[test]
fn malformed_trigger_date_is_a_parse_error() {
    let c = catalog(vec![frcp_pack()]);
    let resp = handle(
        &deadlines_req(&["frcp-test"], D::new("not-a-date").op()),
        &c,
    );
    assert!(!resp.ok);
}

#[test]
fn cross_pack_link_edge_resolves_by_its_origin_nodes_pack() {
    let c = catalog(vec![link_origin_pack(), link_dest_pack(), links_file()]);
    let resp = handle(
        &deadlines_req(
            &["frcp-link-origin", "cofc-link-dest"],
            D::new("2026-01-05")
                .node("frcp-link-origin::leaves")
                .reachable()
                .op(),
        ),
        &c,
    );
    assert!(resp.ok, "{resp:?}");
    let r = resp.result.unwrap();
    assert_eq!(r["count"], json!(1));
    // Origin pack is "frcp-link-origin" -> id-fallback resolves to FRCP 6,
    // not the destination pack's RCFC 6, confirming the origin-pack rule.
    assert_eq!(r["deadlines"][0]["rule_set"], json!("frcp6"));
}
