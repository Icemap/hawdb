// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::suite::Step;

type Result<T> = std::result::Result<T, String>;

fn names(response: &Value) -> Result<Vec<&str>> {
    response["columns"]
        .as_array()
        .ok_or("missing result columns")?
        .iter()
        .map(|name| name.as_str().ok_or_else(|| "invalid column name".into()))
        .collect()
}

fn rows(response: &Value, named: bool, ordered: bool) -> Result<Vec<String>> {
    let columns = names(response)?;
    let mut positions = (0..columns.len()).collect::<Vec<_>>();
    if named {
        positions.sort_by_key(|index| columns[*index]);
    }
    let mut rows = Vec::new();
    for row in response["rows"].as_array().ok_or("missing result rows")? {
        let values = row.as_array().ok_or("invalid result row")?;
        if values.len() != columns.len() {
            return Err("result row/schema width mismatch".into());
        }
        let cells = positions
            .iter()
            .map(|index| {
                let mut cell = values[*index].clone();
                if cell["kind"] == "float" {
                    let text = cell["value"]
                        .as_str()
                        .ok_or("invalid float representation")?;
                    let value = text.parse::<f64>().map_err(|_| "invalid result float")?;
                    cell["value"] = Value::String(if value.is_nan() {
                        "nan".into()
                    } else {
                        format!("{:016x}", value.to_bits())
                    });
                }
                Ok(cell)
            })
            .collect::<Result<Vec<_>>>()?;
        rows.push(serde_json::to_string(&cells).map_err(|e| e.to_string())?);
    }
    // Preserve duplicates. Only unordered SQL results may be sorted.
    if !ordered {
        rows.sort();
    }
    Ok(rows)
}

pub(crate) fn equivalent(pg: &Value, hawdb: &Value, step: &Step) -> Result<bool> {
    if let Some(expected) = &step.expected_error {
        return Ok(pg["status"] == "error"
            && hawdb["status"] == "error"
            && pg["sqlstate"] == expected.pg_sqlstate
            && hawdb["kind"] == expected.hawdb_kind);
    }
    if pg["status"] != "ok" || hawdb["status"] != "ok" {
        return Ok(false);
    }
    let mut left = names(pg)?;
    let mut right = names(hawdb)?;
    if left.len() != right.len() {
        return Ok(false);
    }
    if step.compare_column_names {
        left.sort();
        right.sort();
        if left != right || left.windows(2).any(|pair| pair[0] == pair[1]) {
            return Ok(false);
        }
    }
    Ok(pg["affected"] == hawdb["affected"]
        && rows(pg, step.compare_column_names, step.ordered)?
            == rows(hawdb, step.compare_column_names, step.ordered)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(rows: Value) -> Value {
        json!({"status":"ok","columns":["n"],"rows":rows,"affected":0})
    }
    fn step() -> Step {
        serde_json::from_value(
            json!({"sql":"SELECT 1;", "source":{"file":"x","first_line":1,"last_line":1}}),
        )
        .unwrap()
    }

    #[test]
    fn duplicates_order_and_kinds_cannot_be_normalized_away() {
        let one = json!({"kind":"int","value":"1"});
        let two = json!({"kind":"int","value":"2"});
        let a = response(json!([[one], [one], [two]]));
        let b = response(json!([[two], [one], [one]]));
        assert!(equivalent(&a, &b, &step()).unwrap());
        assert!(!equivalent(&a, &response(json!([[one], [two], [two]])), &step()).unwrap());
        let mut ordered = step();
        ordered.ordered = true;
        assert!(!equivalent(&a, &b, &ordered).unwrap());
        for cell in [
            json!({"kind":"null"}),
            json!({"kind":"bool","value":false}),
            json!({"kind":"text","value":"1"}),
        ] {
            assert!(!equivalent(
                &response(json!([[one]])),
                &response(json!([[cell]])),
                &step()
            )
            .unwrap());
        }
    }

    #[test]
    fn named_associations_width_and_affected_counts_are_checked() {
        let a = json!({"status":"ok","columns":["b","a"],"rows":[[2,1]],"affected":0});
        let mut b = json!({"status":"ok","columns":["a","b"],"rows":[[1,2]],"affected":0});
        assert!(equivalent(&a, &b, &step()).unwrap());
        b["rows"] = json!([[2, 1]]);
        assert!(!equivalent(&a, &b, &step()).unwrap());
        let mut b = response(json!([]));
        b["affected"] = json!(1);
        assert!(!equivalent(&response(json!([])), &b, &step()).unwrap());
        b["columns"] = json!(["n", "m"]);
        b["affected"] = json!(0);
        let mut ordinal = step();
        ordinal.compare_column_names = false;
        assert!(!equivalent(&response(json!([])), &b, &ordinal).unwrap());
    }

    #[test]
    fn float_formats_are_normalized_without_losing_signed_zero() {
        let float = |value| response(json!([[{"kind":"float","value":value}]]));
        assert!(equivalent(&float("1"), &float("1.0"), &step()).unwrap());
        assert!(equivalent(&float("Infinity"), &float("inf"), &step()).unwrap());
        assert!(equivalent(&float("NaN"), &float("nan"), &step()).unwrap());
        assert!(!equivalent(&float("-0"), &float("0"), &step()).unwrap());
    }

    #[test]
    fn only_explicit_typed_error_agreement_passes() {
        let pg = json!({"status":"error","sqlstate":"40001"});
        let mut hawdb = json!({"status":"error","kind":"serialization_failure"});
        let mut s = step();
        assert!(!equivalent(&pg, &hawdb, &s).unwrap());
        s.expected_error = Some(crate::suite::ExpectedError {
            pg_sqlstate: "40001".into(),
            hawdb_kind: "serialization_failure".into(),
        });
        assert!(equivalent(&pg, &hawdb, &s).unwrap());
        hawdb["kind"] = json!("execution");
        assert!(!equivalent(&pg, &hawdb, &s).unwrap());
    }
}
