// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(clippy::unwrap_used)]

use super::*;
use std::collections::HashMap;

struct M(HashMap<&'static str, f64>);

impl Env for M {
    fn var(&self, n: &str) -> Option<f64> {
        self.0.get(n).copied()
    }
    fn func(&self, name: &str, args: &[Arg]) -> Option<Result<f64>> {
        (name == "tag").then(|| args[0].str().map(|s| b(s == "x")))
    }
}

fn env() -> M {
    M(HashMap::from([
        ("hours", 10.0),
        ("rate", 500.0),
        ("fees", 350.0),
        ("p", 0.25),
    ]))
}

fn ev(s: &str) -> f64 {
    parse(s).unwrap().eval(&env()).unwrap()
}

#[test]
fn arithmetic_and_precedence() {
    assert_eq!(ev("hours * rate + fees"), 5350.0);
    assert_eq!(ev("2 + 3 * 4 ^ 2"), 50.0);
    assert_eq!(ev("-2 ^ 2"), -4.0);
    assert_eq!(ev("2 ^ 3 ^ 2"), 512.0);
    assert_eq!(ev("(1 + 2) * 3"), 9.0);
    assert_eq!(ev("1_000 * 2"), 2000.0);
    assert_eq!(ev("1.5e3 + .5"), 1500.5);
    assert_eq!(ev("7 % 4 - 10 / 4"), 0.5);
}

#[test]
fn logic_ternary_functions() {
    assert_eq!(ev("hours > 5 ? 1 : 2"), 1.0);
    assert_eq!(ev("hours > 5 && rate < 100"), 0.0);
    assert_eq!(ev("hours < 5 || rate >= 500"), 1.0);
    assert_eq!(ev("!(hours != 10) && hours <= 10"), 1.0);
    assert_eq!(ev("max(1, hours, 3) + min(4, 2)"), 12.0);
    assert_eq!(ev("clamp(hours, 0, 4)"), 4.0);
    assert!((ev("-ln(p)") - 1.386_294_361_1).abs() < 1e-9);
    assert_eq!(ev("tag('x') * 7 + tag(\"y\")"), 7.0);
    assert_eq!(ev("if(0, 1/0, 3)"), 3.0);
    assert_eq!(ev("hours = 10 ? 1 : 0"), 1.0);
    assert_eq!(
        ev("true + false + isnan(0/0) + step(-1) + default(0/0, 4)"),
        6.0
    );
    assert_eq!(
        ev("abs(-2) + sqrt(16) + pow(2, 3) + log10(100) + exp(0)"),
        17.0
    );
    assert!(ev("inf") > 1e308);
}

#[test]
fn vars_are_collected_once() {
    assert_eq!(
        parse("a + b * a + f(c) + (d ? e : a)").unwrap().vars(),
        ["a", "b", "c", "d", "e"]
    );
}

#[test]
fn errors_are_reported() {
    for bad in ["1 +", "(1", "", "1 2", "a ? b", "'open", "3 # 4", "1..2"] {
        assert!(parse(bad).is_err(), "{bad} should not parse");
    }
    let e = env();
    for bad in [
        "nope",
        "\"s\" + 1",
        "nosuch(1)",
        "if(1, 2)",
        "abs()",
        "min('a')",
        "tag(1)",
    ] {
        assert!(
            parse(bad).unwrap().eval(&e).is_err(),
            "{bad} should not evaluate"
        );
    }
    assert!(Arg::Num(1.0).str().is_err() && Arg::Str("s".into()).num().is_err());
}
