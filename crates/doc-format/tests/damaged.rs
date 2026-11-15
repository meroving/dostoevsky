//! A file's metadata is plain text anyone can edit or break: whatever it says, opening it
//! never panics and gives a valid document.

#[path = "../../doc-model/tests/common/mod.rs"]
mod common;

use common::session_strategy;
use doc_format::{parse, serialize, Preserved};
use proptest::prelude::*;
use serde_json::Value;

/// Changes the record in place, one pick per field: drops fields, nulls them, swaps in a
/// group's id, empties arrays, sets numbers to edge values (`u64::MAX`, huge, -1, 0).
fn damage(v: &mut Value, picks: &mut impl Iterator<Item = u16>) {
    match v {
        Value::Object(o) => {
            let keys: Vec<String> = o.keys().cloned().collect();
            for k in keys {
                match picks.next().unwrap_or(0) % 23 {
                    0 if k != "kind" => {
                        o.remove(&k);
                    }
                    1 => {
                        o.insert(k, Value::from("h1t0p"));
                    }
                    2 => {
                        o.insert(k, Value::Null);
                    }
                    _ => {
                        if let Some(x) = o.get_mut(&k) {
                            damage(x, picks);
                        }
                    }
                }
            }
        }
        Value::Array(a) => {
            match picks.next().unwrap_or(0) % 11 {
                0 => a.clear(),
                1 if !a.is_empty() => {
                    a.remove(0);
                }
                _ => {}
            }
            for x in a {
                damage(x, picks);
            }
        }
        Value::Number(_) => {
            *v = match picks.next().unwrap_or(0) % 10 {
                0 => 0.into(),
                1 => 1.into(),
                2 => 3.into(),
                3 => 999_999.into(),
                4 => (1u64 << 40).into(),
                5 => u64::MAX.into(),
                6 => (-1).into(),
                _ => return,
            };
        }
        Value::String(s) => match picks.next().unwrap_or(0) % 13 {
            0 => s.clear(),
            1 => s.push_str("é\n-->x"),
            _ => {}
        },
        _ => {}
    }
}

fn damaged(file: &str, picks: Vec<u16>) -> String {
    let mut picks = picks.into_iter().cycle();
    file.lines()
        .map(|l| match serde_json::from_str::<Value>(l) {
            Ok(mut v @ Value::Object(_)) => {
                damage(&mut v, &mut picks);
                v.to_string()
            }
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn opens(file: &str) -> Result<(), TestCaseError> {
    let l = parse(file);
    l.doc.check_invariants().map_err(TestCaseError::fail)?;
    let saved = l.serialize();
    parse(&saved).doc.check_invariants().map_err(TestCaseError::fail)?;
    Ok(())
}

#[test]
fn offsets_past_the_end_of_numbers() {
    let f = include_str!("fixtures/spec-example.md")
        .replace("\"offset\":12,", &format!("\"offset\":{},", u64::MAX))
        .replace("\"length\":22,", &format!("\"length\":{},", u64::MAX));
    opens(&f).unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(1000),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn damaged_metadata_opens(session in session_strategy(), picks in prop::collection::vec(any::<u16>(), 1..64)) {
        let d = session.build();
        opens(&damaged(&serialize(&d, true, &Preserved::default()), picks))?;
    }

    #[test]
    fn the_damaged_spec_example_opens(picks in prop::collection::vec(any::<u16>(), 1..64)) {
        opens(&damaged(include_str!("fixtures/spec-example.md"), picks))?;
    }
}
