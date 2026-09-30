// SPDX-License-Identifier: GPL-3.0-or-later
//! Famous cases, replayed. Each `tests/cases/<slug>.json` maps a real,
//! sourced procedural history onto pack nodes and edges. For every case this
//! asserts that:
//!
//! 1. the history is a valid walk in the compiled graph: every step is an
//!    out-edge of the node the case is on, so links and state flags are
//!    honored exactly as every algorithm sees them;
//! 2. every documented gap (a real step no pack can express) is still a gap.
//!    Once someone closes one, the test fails and says to merge the legs;
//! 3. the walk ends on the expected node, with the expected outcome tags
//!    and state flags;
//! 4. the `deadlines` op computes each key deadline to the hand-computed
//!    date under the governing rule. That is the rule's date, not the day
//!    the parties actually filed;
//! 5. the rendered story matches `tests/cases/<slug>.story.txt`, an
//!    `expect-test` snapshot a reviewer can read like a case summary.
//!    Regenerate with `UPDATE_EXPECT=1 cargo test --test famous_cases`.
//!
//! See `tests/README.md` for how this layer fits with the others.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "famous_cases/case.rs"]
mod case;
#[path = "famous_cases/render.rs"]
mod render;

use case::{Case, Walked};
use expect_test::{expect_file, ExpectFile};
use litgraph::api::{handle, Catalog, Op, Request};
use litgraph::model::Graph;

/// Run each deadline check through the real `deadlines` op and return one
/// rendered line per check.
fn check_deadlines(g: &Graph, case: &Case, walked: &[Walked]) -> Vec<String> {
    let catalog = Catalog::embedded().unwrap();
    let on_path: Vec<&str> = walked
        .iter()
        .flat_map(|w| &w.taken)
        .map(|t| g.edges[t.edge].base_id.as_str())
        .collect();
    let mut lines = vec![];
    for d in &case.deadlines {
        assert!(
            on_path.contains(&d.edge.as_str()),
            "{}: deadline edge {} is not one of the walked steps",
            case.name,
            d.edge
        );
        let req = Request {
            packs: case.packs.clone(),
            links: true,
            op: Op::Deadlines {
                trigger: d.trigger.clone(),
                edge: Some(d.edge.clone()),
                node: None,
                reachable: false,
                service_method: None,
                additional_holidays: vec![],
                clerk_inaccessible: false,
            },
            ..Request::default()
        };
        let resp = handle(&req, &catalog);
        assert!(resp.ok, "{}: deadlines op failed: {resp:?}", case.name);
        let r = &resp.result.unwrap()["deadlines"][0];
        assert_eq!(r["rule_set"], d.rule_set, "{}: {}", case.name, d.edge);
        assert_eq!(
            r["due_date"], d.due,
            "{}: {} from {} (steps: {})",
            case.name, d.edge, d.trigger, r["steps"]
        );
        lines.push(format!(
            "{}\n    {} days under {}: {} ({}) -> due {}\n    arithmetic: {}\n    historical: {}",
            d.edge,
            r["deadline_length"].as_f64().unwrap_or_default(),
            r["rule_set_label"].as_str().unwrap_or_default(),
            d.trigger,
            d.trigger_event,
            d.due,
            d.arithmetic,
            d.historical
        ));
    }
    lines
}

/// Every structural assertion about one case, then the story snapshot.
fn check(slug: &str, snapshot: &ExpectFile) {
    let case = case::load(slug);
    let g = case::compile(&case);
    let walked: Vec<Walked> = (0..case.legs.len())
        .map(|i| case::walk(&g, &case, i))
        .collect();

    // Every leg but the last ends where a documented gap begins; a gap
    // begins only where some leg ends.
    let ends: Vec<&str> = walked
        .iter()
        .map(|w| g.nodes[case::end(w)].base_id.as_str())
        .collect();
    for (i, e) in ends.iter().enumerate().take(ends.len() - 1) {
        assert!(
            case.gaps.iter().any(|gap| gap.from == *e),
            "{}: leg {} ends at {e} but no gap explains the jump to leg {}",
            case.name,
            i + 1,
            i + 2
        );
    }
    for gap in &case.gaps {
        assert!(
            ends.contains(&gap.from.as_str()),
            "{}: gap '{}' starts at {}, where no leg ends",
            case.name,
            gap.event,
            gap.from
        );
        assert!(
            !case::gap_is_closed(&g, gap),
            "{}: gap '{}' ({} -> {:?}) is now expressible. Merge the legs \
             around it and delete it from `gaps`.",
            case.name,
            gap.event,
            gap.from,
            gap.to
        );
    }

    let last = case::end(walked.last().unwrap());
    let node = &g.nodes[last];
    assert_eq!(node.base_id, case.outcome.node, "{}: final node", case.name);
    assert!(
        node.is_terminal() || g.out[last].iter().any(|&e| g.edges[e].synthetic),
        "{}: {} is not a terminal (or a continued terminal)",
        case.name,
        node.id
    );
    assert_eq!(
        node.outcome, case.outcome.tags,
        "{}: outcome tags",
        case.name
    );
    assert_eq!(node.flags, case.outcome.flags, "{}: state flags", case.name);

    let ids: Vec<&str> = case.sources.iter().map(|s| s.id.as_str()).collect();
    for s in case.legs.iter().flat_map(|l| &l.steps) {
        if let Some(src) = &s.source {
            assert!(
                ids.contains(&src.as_str()),
                "{}: unknown source {src}",
                case.name
            );
        }
    }

    let deadlines = check_deadlines(&g, &case, &walked);
    snapshot.assert_eq(&render::story(&g, &case, &walked, &deadlines));
}

macro_rules! famous_case {
    ($name:ident, $slug:literal) => {
        #[test]
        fn $name() {
            check(
                $slug,
                &expect_file![concat!("../../../tests/cases/", $slug, ".story.txt")],
            );
        }
    };
}

famous_case!(
    twombly_rule_12b6_dismissal_reaches_cert_through_the_second_circuit,
    "twombly"
);
famous_case!(
    iqbal_denied_immunity_motion_reaches_cert_through_a_collateral_order_appeal,
    "iqbal"
);
famous_case!(
    celotex_summary_judgment_reaches_cert_through_the_dc_circuit,
    "celotex"
);
famous_case!(
    ebay_injunction_denial_reaches_cert_through_the_federal_circuit,
    "ebay"
);
famous_case!(
    oil_states_ipr_reaches_cert_after_a_rule_36_affirmance,
    "oil-states"
);
famous_case!(
    sas_partial_institution_reaches_cert_after_a_mixed_decision,
    "sas"
);
famous_case!(
    arthrex_goes_to_the_supreme_court_and_back_through_director_review,
    "arthrex"
);
famous_case!(
    hughes_1498_case_is_remanded_for_damages_then_gvrd_then_affirmed,
    "hughes"
);
famous_case!(
    blue_and_gold_bid_protest_denial_is_affirmed_without_cert,
    "blue-and-gold"
);
famous_case!(
    suprema_itc_exclusion_order_survives_en_banc_rehearing,
    "suprema"
);
