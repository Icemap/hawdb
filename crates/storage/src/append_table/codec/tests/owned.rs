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
use crate::background::{CheckpointDecodeContext, CheckpointWorkContext};
use crate::relational::{RelationalColumnDefault, RelationalOverflowRef};
use hawdb_core::{RuntimeTaskContext, Uuid};
use hawdb_qos::{
    IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeMemorySnapshot,
    RuntimeResourceBudget, RuntimeResourceSnapshot,
};
use std::num::NonZeroUsize;

fn governor() -> RuntimeGovernor {
    let governor = RuntimeGovernor::new(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(16 * 1024 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        RuntimeResourceSnapshot::from_parts(
            RuntimeResourceBudget::from_limits(NonZeroUsize::MIN, None, None),
            RuntimeMemorySnapshot::from_limits(Some(1 << 30), Some(1 << 30), None, None, None),
        ),
        IoConcurrencyBudget::new(2, 1),
    );
    governor.pin_resources();
    governor
}

fn compare(
    governor: &RuntimeGovernor,
    encoded: &[u8],
    limits: AppendDecodeLimits,
) -> Result<AppendWalBatch, AppendTableError> {
    let reference = decode_append_wal_batch(encoded, limits);
    let admission = governor
        .try_admit_incremental_maintenance(0, 16 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let owned = CheckpointDecodeContext {
        work: CheckpointWorkContext::new(admission.task_context().unwrap().clone()),
        memory: std::cell::RefCell::default(),
    };
    let controlled = decode_batch(encoded, limits, Some(&owned));
    assert_eq!(controlled, reference, "{} bytes, {limits:?}", encoded.len());
    if controlled.is_ok() {
        assert!(admission.memory_report().live_accounted_bytes > 0);
    }
    drop(controlled);
    drop(owned);
    assert_eq!(admission.memory_report().live_accounted_bytes, 0);
    drop(admission);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    reference
}

fn generated(seed: u64) -> AppendTransaction {
    let mut transaction = transaction();
    let AppendWrite::CreateTable { schema } = &mut transaction.writes[0] else {
        unreachable!()
    };
    schema.order_mode = if seed.is_multiple_of(2) {
        AppendOrderMode::CallerProvided
    } else {
        AppendOrderMode::CommitSequence
    };
    let values = vec![
        RelationalValue::Boolean(seed.is_multiple_of(2)),
        RelationalValue::BigInt(-(seed as i64)),
        RelationalValue::DoublePrecision(seed as f64 / 7.0),
        RelationalValue::Text("界🙂".repeat(seed as usize)),
        RelationalValue::Bytea(vec![seed as u8; seed as usize * 3]),
        RelationalValue::Uuid(Uuid::from_u128(seed as u128)),
    ];
    for (index, value) in values.iter().enumerate() {
        schema.columns.push(RelationalColumnSchema {
            name: format!("payload_{index}"),
            scalar_type: value.scalar_type().unwrap(),
            nullable: true,
            default: Some(RelationalColumnDefault::Literal(value.clone())),
        });
    }
    schema.columns.push(RelationalColumnSchema {
        name: "null_default".into(),
        scalar_type: RelationalScalarType::Text,
        nullable: true,
        default: Some(RelationalColumnDefault::Literal(RelationalValue::Null)),
    });
    let AppendWrite::Append { rows, .. } = &mut transaction.writes[1] else {
        unreachable!()
    };
    let mut row = rows[0].values().to_vec();
    row.extend(values);
    row.push(RelationalValue::Null);
    *rows = vec![RelationalRow::new(row)];
    // Exercise the durable overflow tag without requiring hydration. Append
    // row payloads have the same reference semantics as ordinary decoding.
    rows.push(RelationalRow::new(vec![RelationalValue::Overflow(
        RelationalOverflowRef {
            digest: integrity_digest(&seed.to_le_bytes()).sha256,
            scalar_type: RelationalScalarType::Bytea,
            compressed_bytes: seed + 1,
            uncompressed_bytes: seed + 100,
        },
    )]));
    transaction
}

#[test]
fn owned_append_wal_matches_generated_schema_defaults_and_all_value_tags() {
    let governor = governor();
    for seed in 0..32 {
        let transaction = generated(seed);
        let encoded = encode_append_wal_batch(seed, &transaction).unwrap();
        let decoded = compare(&governor, &encoded, AppendDecodeLimits::wal()).unwrap();
        assert_eq!(decoded.epoch, seed);
        assert_eq!(decoded.transaction, transaction);
    }
}

#[test]
fn owned_append_wal_matches_rechecksummed_corruption_and_every_torn_boundary() {
    let governor = governor();
    let encoded = encode_append_wal_batch(7, &generated(7)).unwrap();
    let limits = AppendDecodeLimits {
        max_record_bytes: 8192,
        max_writes: 64,
        max_rows: 64,
        max_values: 64,
        max_value_bytes: 4096,
        max_schema_bytes: 4096,
    };
    assert!(compare(&governor, &encoded, limits).is_ok());
    for end in 0..encoded.len() {
        assert!(compare(&governor, &encoded[..end], limits).is_err());
    }
    let payload = &encoded[APPEND_HEADER_BYTES..];
    let mut rejected = 0;
    for offset in 0..payload.len() {
        let mut mutated = payload.to_vec();
        mutated[offset] ^= 0x80;
        let valid_envelope = encode_envelope(7, mutated).unwrap();
        if compare(&governor, &valid_envelope, limits).is_err() {
            rejected += 1;
        }
        let torn_payload = encode_envelope(7, payload[..offset].to_vec()).unwrap();
        assert!(compare(&governor, &torn_payload, limits).is_err());
    }
    assert!(rejected > 100);
    println!(
        "compared {} corrupt payload positions and every torn boundary",
        payload.len()
    );
}

#[test]
fn owned_append_wal_preserves_each_decoder_limit_boundary() {
    let governor = governor();
    let encoded = encode_append_wal_batch(13, &generated(13)).unwrap();
    let ordinary = AppendDecodeLimits::wal();
    for limit in 0..64 {
        for limits in [
            AppendDecodeLimits {
                max_record_bytes: limit,
                ..ordinary
            },
            AppendDecodeLimits {
                max_writes: limit,
                ..ordinary
            },
            AppendDecodeLimits {
                max_rows: limit,
                ..ordinary
            },
            AppendDecodeLimits {
                max_values: limit,
                ..ordinary
            },
            AppendDecodeLimits {
                max_value_bytes: limit,
                ..ordinary
            },
            AppendDecodeLimits {
                max_schema_bytes: limit,
                ..ordinary
            },
        ] {
            let _ = compare(&governor, &encoded, limits);
        }
    }
}

#[test]
fn owned_append_wal_preserves_rejection_of_uuidv7_defaults() {
    let governor = governor();
    let mut transaction = generated(2);
    let AppendWrite::CreateTable { schema } = &mut transaction.writes[0] else {
        unreachable!()
    };
    schema.columns[7].default = Some(RelationalColumnDefault::UuidV7);
    // A valid relational schema envelope can still contain a default that
    // append tables reject. Both decoders must reach the append validator.
    let limits = AppendDecodeLimits::wal();
    let mut payload = Encoder::default();
    payload.count(1, "writes").unwrap();
    payload.u8(CREATE_TABLE_TAG);
    encode_schema(&mut payload, schema, limits).unwrap();
    let encoded = encode_envelope(2, payload.finish()).unwrap();
    assert!(matches!(
        encode_append_wal_batch(2, &transaction),
        Err(AppendTableError::Schema(_))
    ));
    assert!(matches!(
        compare(&governor, &encoded, limits),
        Err(AppendTableError::Schema(_))
    ));
}
