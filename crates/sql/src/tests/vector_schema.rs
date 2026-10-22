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

#[test]
fn create_and_alter_preserve_dimensions_and_nullability() {
    for size in [1, 1024, u32::MAX] {
        let kind = SqlDataType::Vector(VectorDimension::new(size).unwrap());
        for spelling in ["vector", "VECTOR", "\"vector\""] {
            let SqlStatement::CreateTable(create) = parse_postgres_sql(&format!(
                "CREATE TABLE memories (id TEXT PRIMARY KEY, embedding {spelling}({size}) NOT NULL)"
            ))
            .unwrap() else {
                panic!("expected CREATE TABLE")
            };
            assert_eq!(create.columns[1].data_type, kind);
            assert!(!create.columns[1].nullable);
        }
        let SqlStatement::AlterTableAddColumn(alter) = parse_postgres_sql(&format!(
            "ALTER TABLE memories ADD COLUMN embedding VECTOR({size})"
        ))
        .unwrap() else {
            panic!("expected ALTER TABLE")
        };
        assert_eq!(alter.column.data_type, kind);
        assert!(alter.column.nullable);
    }
}

#[test]
fn invalid_dimensions_and_unsupported_type_spellings_are_rejected() {
    for kind in [
        "VECTOR",
        "VECTOR()",
        "VECTOR(0)",
        "VECTOR(-1)",
        "VECTOR(1, 2)",
        "VECTOR(1.5)",
        "VECTOR(4294967296)",
        "VECTOR(99999999999999999999)",
        "VECTOR($1)",
        "VECTOR('3')",
        "VECTOR(,3)",
        "VECTOR(3,)",
        "VECTOR(,,3,,)",
        "VECTOR(3)[]",
        "public.vector(3)",
        "\"VECTOR\"(3)",
    ] {
        for sql in [
            format!("CREATE TABLE memories (id TEXT PRIMARY KEY, embedding {kind})"),
            format!("ALTER TABLE memories ADD COLUMN embedding {kind}"),
        ] {
            assert!(parse_postgres_sql(&sql).is_err(), "{sql}");
        }
    }
}

#[test]
fn source_validation_only_checks_vector_type_locations() {
    parse_postgres_sql(
        "-- VECTOR('3')\nCREATE TABLE memories (id TEXT PRIMARY KEY, note TEXT DEFAULT 'VECTOR(,3)', embedding VECTOR /* comment */ (3), other VECTOR(4))"
    ).unwrap();
    parse_postgres_sql("CREATE TABLE \"vector\" (id TEXT PRIMARY KEY, vector TEXT)").unwrap();
}
