// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn shared_vector_json_parameters_preserve_type_and_reject_invalid_values() {
    let value = Value::Vector(hawdb::Vector::new(&[1.0, -0.0, 0.5]).unwrap());
    assert_eq!(json_to_value(&value_to_json(&value)).unwrap(), value);
    for value in [
        json!({"$vector": []}),
        json!({"$vector": [1e300]}),
        json!({"$vector": ["x"]}),
        json!({"$vector": null}),
    ] {
        assert!(json_to_value(&value).is_err(), "{value}");
    }
    assert!(matches!(
        json_to_value(&json!([1.0, 0.0])).unwrap(),
        Value::List(_)
    ));
}
