// SPDX-License-Identifier: GPL-3.0-or-later
//! Defaults and deserializers named by `op.rs`'s serde attributes.

use serde::{Deserialize, Deserializer};

use super::op::Op;

pub(super) fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        OneOrMany::Many(v) => v,
    })
}

pub(super) fn v(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}
pub(super) fn chain_metrics() -> Vec<String> {
    v(&["dollars", "elapsed", "hours"])
}
pub(super) fn sim_metrics() -> Vec<String> {
    v(&["dollars", "elapsed"])
}
pub(super) fn pareto_objectives() -> Vec<String> {
    v(&["dollars", "elapsed", "surprise"])
}
pub(super) fn tornado_params() -> Vec<String> {
    v(&["rate", "stakes"])
}
pub(super) fn terminals() -> String {
    "terminals".into()
}
pub(super) fn dollars() -> String {
    "dollars".into()
}
pub(super) fn one() -> String {
    "1".into()
}
pub(super) fn yes() -> bool {
    true
}
pub(super) const fn n<const N: usize>() -> usize {
    N
}
pub(super) fn f_100() -> f64 {
    100.0
}
pub(super) fn f_1500() -> f64 {
    1500.0
}
pub(super) fn f_tol() -> f64 {
    1e-3
}
pub(super) fn f_alpha() -> f64 {
    0.1
}
pub(super) fn f_rel() -> f64 {
    0.25
}
pub(super) fn f_dp() -> f64 {
    0.1
}
pub(super) fn f_credibility() -> f64 {
    0.9
}
pub(super) fn seed() -> u64 {
    7
}
// serde's `default = "chain_op"` requires this to return exactly the
// field's type (`Box<Op>`); returning `Op` would not satisfy that bound.
#[allow(clippy::unnecessary_box_returns)]
pub(super) fn chain_op() -> Box<Op> {
    Box::new(Op::Chain {
        from: None,
        metrics: chain_metrics(),
        top: 15,
    })
}
