// Copyright 2026 Nowledge
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use hawdb_core::VectorDimension;

fn schema(dimension: u32) -> RelationalTableSchema {
    RelationalTableSchema {
        name: "memories".into(),
        columns: vec![
            RelationalColumnSchema {
                name: "id".into(),
                scalar_type: RelationalScalarType::BigInt,
                nullable: false,
                default: None,
            },
            RelationalColumnSchema {
                name: "embedding".into(),
                scalar_type: RelationalScalarType::Vector(VectorDimension::new(dimension).unwrap()),
                nullable: true,
                default: Some(RelationalColumnDefault::Literal(RelationalValue::Null)),
            },
        ],
        primary_key: vec!["id".into()],
        unique_constraints: vec![],
        foreign_keys: vec![],
        indexes: vec![],
    }
}

#[test]
fn scalar_tags_stay_stable_and_vector_dimensions_are_checked_on_decode() {
    for (kind, tag) in [
        (RelationalScalarType::Boolean, 1),
        (RelationalScalarType::BigInt, 2),
        (RelationalScalarType::DoublePrecision, 3),
        (RelationalScalarType::Text, 4),
        (RelationalScalarType::Bytea, 5),
        (RelationalScalarType::Uuid, 6),
    ] {
        let mut encoder = Encoder::default();
        encoder.scalar_type(kind);
        assert_eq!(encoder.finish(), [tag]);
    }
    let encoded = [7, 0, 4, 0, 0];
    let mut decoder = Decoder::from_slice(&encoded, RelationalDecodeLimits::wal(), true);
    assert_eq!(
        decoder.scalar_type().unwrap(),
        schema(1024).columns[1].scalar_type
    );
    decoder.finish().unwrap();
    for end in 0..encoded.len() {
        assert!(
            Decoder::from_slice(&encoded[..end], RelationalDecodeLimits::wal(), true)
                .scalar_type()
                .is_err()
        );
    }
    assert!(matches!(
        Decoder::from_slice(&[7, 0, 0, 0, 0], RelationalDecodeLimits::wal(), true).scalar_type(),
        Err(RelationalError::Corruption(_))
    ));
}

#[test]
fn schema_round_trip_preserves_maximum_dimension_without_payload_allocation() {
    for dimension in [1, 1024, u32::MAX] {
        let expected = schema(dimension);
        let encoded = encode_relational_table_schema(&expected).unwrap();
        assert!(encoded.len() < 256);
        assert_eq!(
            decode_relational_table_schema(&encoded, 256, 16).unwrap(),
            expected
        );
        for end in 0..encoded.len() {
            assert!(decode_relational_table_schema(&encoded[..end], 256, 16).is_err());
        }
    }
}
