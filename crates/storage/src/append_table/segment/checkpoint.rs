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

//! Cooperative construction of the existing V2 append segment.
//!
//! Rows, blocks and copies have separate cancellation/admission boundaries.
//! Complete buffers, variable-sized key comparisons and Arc conversion still
//! need the owner's resource reservation; this is not a byte-ledger substitute.

mod compaction;
mod descriptors;
mod encoder;
mod externalized;
mod overflows;
use descriptors::CheckpointDescriptors;
use encoder::CheckpointEncoder;
use externalized::{CheckpointExternalizedOverflows, CheckpointExternalizedRow};
use overflows::CheckpointOverflows;
pub(super) mod read;

use super::*;
use crate::append_table::checkpoint::work_error;
use crate::background::{CheckpointBytes, CheckpointSharedBytes, CheckpointWorkContext};
use std::io::Write;

#[derive(Debug)]
pub(in crate::append_table) struct CheckpointSegmentWriteOutput {
    pub(in crate::append_table) generation: u64,
    pub(in crate::append_table) source_commit_epoch: u64,
    pub(in crate::append_table) encoded: CheckpointSharedBytes,
    pub(in crate::append_table) descriptors: CheckpointDescriptors,
    pub(in crate::append_table) artifact: AppendSegmentArtifactMetadata,
}

impl PartialEq<AppendSegmentWriteOutput> for CheckpointSegmentWriteOutput {
    fn eq(&self, other: &AppendSegmentWriteOutput) -> bool {
        self.generation == other.generation
            && self.source_commit_epoch == other.source_commit_epoch
            && self.encoded.as_ref() == other.encoded.as_ref()
            && self.descriptors == other.descriptors
            && self.artifact == other.artifact
    }
}

pub(super) fn encode(
    generation: u64,
    source_commit_epoch: u64,
    rows: &[AppendTableRow],
    config: AppendSegmentConfig,
    work: &CheckpointWorkContext,
) -> Result<CheckpointSegmentWriteOutput, AppendTableError> {
    work.checkpoint().map_err(work_error)?;
    validate_config(config)?;
    if rows.len() > config.max_rows {
        return Err(AppendTableError::Admission(format!(
            "append segment contains {} rows, exceeding limit {}",
            rows.len(),
            config.max_rows
        )));
    }
    for pair in rows.windows(2) {
        let unit = work.start_unit().map_err(work_error)?;
        if !compare_rows(&pair[0], &pair[1]).is_lt() {
            return Err(AppendTableError::Constraint(
                "append segment rows must be strictly ordered by table, partition, and order key"
                    .to_string(),
            ));
        }
        unit.finish();
    }

    let mut descriptors = CheckpointDescriptors::default();
    let mut payload = CheckpointBytes::new(0, work).map_err(work_error)?;
    let mut start = 0;
    let mut partition_end = 0;
    while start < rows.len() {
        let first = &rows[start];
        if start == partition_end {
            partition_end = start + 1;
            while partition_end < rows.len() {
                let unit = work.start_unit().map_err(work_error)?;
                let row = &rows[partition_end];
                let same = row.table == first.table && row.partition_key == first.partition_key;
                unit.finish();
                if !same {
                    break;
                }
                partition_end += 1;
            }
        }
        let (end, raw) = encode_block(rows, start, partition_end, config, work)?;
        if descriptors.len() == config.max_blocks {
            return Err(AppendTableError::Admission(format!(
                "append segment block count exceeds limit {}",
                config.max_blocks
            )));
        }
        if raw.len() > config.max_decoded_block_bytes {
            return Err(AppendTableError::Admission(format!(
                "append block contains {} decoded bytes, exceeding limit {}",
                raw.len(),
                config.max_decoded_block_bytes
            )));
        }
        let compressed = compress(&raw, config.compression_level, work)?;
        if compressed.len() > config.max_compressed_block_bytes {
            return Err(AppendTableError::Admission(format!(
                "append block contains {} compressed bytes, exceeding limit {}",
                compressed.len(),
                config.max_compressed_block_bytes
            )));
        }
        let payload_offset = u64::try_from(payload.len()).map_err(|_| {
            AppendTableError::Admission("append payload offset overflows u64".to_string())
        })?;
        let digest = work.integrity(&compressed).map_err(work_error)?;
        payload
            .append_growing(&compressed, None, work)
            .map_err(work_error)?;
        let table = descriptors.copy_table(&first.table, work)?;
        let partition_key = descriptors.copy_key(&first.partition_key, work)?;
        let min_order_key = descriptors.copy_key(&first.order_key, work)?;
        let max_order_key = descriptors.copy_key(&rows[end - 1].order_key, work)?;
        descriptors.push(
            AppendSegmentBlockDescriptor {
                table,
                partition_key,
                min_order_key,
                max_order_key,
                row_count: u32::try_from(end - start).map_err(|_| {
                    AppendTableError::Admission("append block row count overflows u32".to_string())
                })?,
                payload_offset,
                compressed_bytes: u64::try_from(compressed.len()).map_err(|_| {
                    AppendTableError::Admission(
                        "append compressed block length overflows u64".to_string(),
                    )
                })?,
                decoded_bytes: u64::try_from(raw.len()).map_err(|_| {
                    AppendTableError::Admission(
                        "append decoded block length overflows u64".to_string(),
                    )
                })?,
                payload_crc32c: digest.crc32c.get(),
                payload_sha256: digest.sha256,
            },
            work,
        )?;
        start = end;
    }

    let directory = directory(&descriptors, config, work)?;
    let total_len = SEGMENT_HEADER_BYTES
        .checked_add(directory.len())
        .and_then(|len| len.checked_add(payload.len()))
        .ok_or_else(|| AppendTableError::Admission("append segment size overflow".to_string()))?;
    if total_len > config.max_segment_bytes {
        return Err(AppendTableError::Admission(format!(
            "append segment contains {total_len} bytes, exceeding limit {}",
            config.max_segment_bytes
        )));
    }
    let body_digest = integrity_slices(&[&directory, &payload], work)?;
    let mut header = CheckpointEncoder::new(work)?;
    header.raw(SEGMENT_MAGIC)?;
    header.raw(&SEGMENT_VERSION.to_le_bytes())?;
    header.raw(&0u16.to_le_bytes())?;
    header.u64(generation)?;
    header.u64(source_commit_epoch)?;
    header.u64(descriptors.len() as u64)?;
    header.u64(directory.len() as u64)?;
    header.u64(payload.len() as u64)?;
    header.u32(body_digest.crc32c.get())?;
    header.raw(body_digest.sha256.as_bytes())?;
    header.u64(0)?;
    let header = header.finish();
    debug_assert_eq!(header.len(), SEGMENT_HEADER_BYTES);
    let encoded = CheckpointSharedBytes::build(total_len, work, |encoded| {
        encoded.append(&header, work)?;
        encoded.append(&directory, work)?;
        encoded.append(&payload, work)
    })
    .map_err(work_error)?;
    let artifact_digest = work.integrity(&encoded).map_err(work_error)?;
    let unit = work.start_unit().map_err(work_error)?;
    let artifact = AppendSegmentArtifactMetadata {
        encoded_len: encoded.len() as u64,
        encoded_crc32c: artifact_digest.crc32c.get(),
        encoded_sha256: artifact_digest.sha256,
    };
    let output = CheckpointSegmentWriteOutput {
        generation,
        source_commit_epoch,
        encoded,
        descriptors,
        artifact,
    };
    unit.finish();
    work.checkpoint().map_err(work_error)?;
    Ok(output)
}

fn encode_block(
    rows: &[AppendTableRow],
    start: usize,
    partition_end: usize,
    config: AppendSegmentConfig,
    work: &CheckpointWorkContext,
) -> Result<(usize, crate::background::CheckpointBytes), AppendTableError> {
    let mut encoder = CheckpointEncoder::new(work)?;
    encoder.u32(0)?;
    let mut overflows = CheckpointOverflows::default();
    let mut end = start;
    while end < partition_end {
        let row = &rows[end];
        let order_key = key(&row.order_key, config.max_key_bytes, work)?;
        let (externalized, row_overflows) = externalize(&row.row, config, work)?;
        let row_payload =
            crate::relational::encode_relational_row_payload_with_work_context(&externalized, work)
                .map_err(map_relational)?;
        if row_payload.len() > config.max_row_bytes {
            return Err(AppendTableError::Admission(format!(
                "append row contains {} bytes, exceeding limit {}",
                row_payload.len(),
                config.max_row_bytes
            )));
        }
        let mut new_overflow_bytes = 0usize;
        for overflow in row_overflows.iter() {
            let unit = work.start_unit().map_err(work_error)?;
            if overflows.contains_key(&overflow.reference.digest) {
                unit.finish();
                continue;
            }
            new_overflow_bytes = new_overflow_bytes
                .checked_add(SHA256_BYTES + 8)
                .and_then(|bytes| bytes.checked_add(overflow.bytes.len()))
                .ok_or_else(|| {
                    AppendTableError::Admission("append block size overflow".to_string())
                })?;
            unit.finish();
        }
        let next_len = encoder
            .len()
            .checked_add(16)
            .and_then(|len| len.checked_add(order_key.len()))
            .and_then(|len| len.checked_add(row_payload.len()))
            .and_then(|len| len.checked_add(new_overflow_bytes))
            .ok_or_else(|| AppendTableError::Admission("append block size overflow".to_string()))?;
        if end > start && next_len > config.target_decoded_block_bytes {
            break;
        }
        encode_bytes(&mut encoder, &order_key, "append order key", work)?;
        encode_bytes(&mut encoder, &row_payload, "append row payload", work)?;
        for overflow in row_overflows.iter() {
            let existing = overflows.insert(overflow.clone(), work)?;
            if let Some(existing) = existing {
                let same = existing.reference == overflow.reference
                    && bytes_equal(&existing.bytes, &overflow.bytes, work)?;
                if !same {
                    return Err(AppendTableError::Corruption(
                        "append overflow digest has conflicting envelopes".to_string(),
                    ));
                }
            }
        }
        end += 1;
    }
    let unit = work.start_unit().map_err(work_error)?;
    let row_count = u32::try_from(end - start).map_err(|_| {
        AppendTableError::Admission("append block row count exceeds durable limit".to_string())
    })?;
    encoder.overwrite_u32(0, row_count);
    let overflow_count = u32::try_from(overflows.len()).map_err(|_| {
        AppendTableError::Admission("append overflow count exceeds durable limit".to_string())
    })?;
    unit.finish();
    encoder.u32(overflow_count)?;
    for (digest, overflow) in overflows.iter() {
        encoder.raw(digest.as_bytes())?;
        encode_bytes(
            &mut encoder,
            &overflow.bytes,
            "append overflow envelope",
            work,
        )?;
    }
    work.checkpoint().map_err(work_error)?;
    Ok((end, encoder.finish()))
}

fn externalize<'a>(
    row: &'a RelationalRow,
    config: AppendSegmentConfig,
    work: &CheckpointWorkContext,
) -> Result<
    (
        CheckpointExternalizedRow<'a>,
        CheckpointExternalizedOverflows,
    ),
    AppendTableError,
> {
    let mut needs_externalization = false;
    for value in row.values() {
        let unit = work.start_unit().map_err(work_error)?;
        needs_externalization = match value {
            RelationalValue::Text(value) => value.len() >= config.overflow_threshold_bytes,
            RelationalValue::Bytea(value) => value.len() >= config.overflow_threshold_bytes,
            // Keep unresolved-reference errors in the existing conversion
            // order, including errors from earlier oversized values.
            RelationalValue::Overflow(_) => true,
            _ => false,
        };
        unit.finish();
        work.checkpoint().map_err(work_error)?;
        if needs_externalization {
            break;
        }
    }
    if !needs_externalization {
        return Ok((
            CheckpointExternalizedRow {
                row: std::borrow::Cow::Borrowed(row),
                _allocations: crate::background::CheckpointAllocationOwner::default(),
            },
            CheckpointExternalizedOverflows::default(),
        ));
    }
    let overflow_config = RelationalOverflowConfig {
        threshold_bytes: config.overflow_threshold_bytes,
        compression_level: config.overflow_compression_level,
        max_value_bytes: config.max_row_bytes,
    };
    let mut allocations = crate::background::CheckpointAllocationOwner::default();
    let mut values = externalized::row_values(row.values().len(), &mut allocations, work)?;
    let mut overflows = CheckpointExternalizedOverflows::new(row.values().len(), work)?;
    for value in row.values() {
        work.checkpoint().map_err(work_error)?;
        let encoded = match value {
            RelationalValue::Text(value) if value.len() >= config.overflow_threshold_bytes => Some(
                crate::relational::overflow::encode_overflow_envelope_with_work_context(
                    RelationalScalarType::Text,
                    value.as_bytes(),
                    overflow_config,
                    work,
                )
                .map_err(map_relational)?,
            ),
            RelationalValue::Bytea(value) if value.len() >= config.overflow_threshold_bytes => {
                Some(
                    crate::relational::overflow::encode_overflow_envelope_with_work_context(
                        RelationalScalarType::Bytea,
                        value,
                        overflow_config,
                        work,
                    )
                    .map_err(map_relational)?,
                )
            }
            RelationalValue::Overflow(_) => {
                return Err(AppendTableError::Constraint(
                    "append input rows cannot contain unresolved overflow references".to_string(),
                ));
            }
            _ => None,
        };
        let value = if let Some(encoded) = encoded {
            let unit = work.start_unit().map_err(work_error)?;
            let value = RelationalValue::Overflow(encoded.reference);
            unit.finish();
            overflows.push(encoded, work)?;
            value
        } else {
            crate::append_table::checkpoint::memory::copy_value(value, &mut allocations, work)?
        };
        let unit = work.start_unit().map_err(work_error)?;
        values.push(value);
        unit.finish();
    }
    let unit = work.start_unit().map_err(work_error)?;
    let output = (
        CheckpointExternalizedRow {
            row: std::borrow::Cow::Owned(RelationalRow::new(values)),
            _allocations: allocations,
        },
        overflows,
    );
    unit.finish();
    work.checkpoint().map_err(work_error)?;
    Ok(output)
}

fn directory(
    descriptors: &[AppendSegmentBlockDescriptor],
    config: AppendSegmentConfig,
    work: &CheckpointWorkContext,
) -> Result<crate::background::CheckpointBytes, AppendTableError> {
    let mut encoder = CheckpointEncoder::new(work)?;
    for descriptor in descriptors {
        encode_bytes(
            &mut encoder,
            descriptor.table.as_bytes(),
            "append block table",
            work,
        )?;
        encode_bytes(
            &mut encoder,
            &key(&descriptor.partition_key, config.max_key_bytes, work)?,
            "append partition key",
            work,
        )?;
        encode_bytes(
            &mut encoder,
            &key(&descriptor.min_order_key, config.max_key_bytes, work)?,
            "append minimum order key",
            work,
        )?;
        encode_bytes(
            &mut encoder,
            &key(&descriptor.max_order_key, config.max_key_bytes, work)?,
            "append maximum order key",
            work,
        )?;
        encoder.u32(descriptor.row_count)?;
        encoder.u64(descriptor.payload_offset)?;
        encoder.u64(descriptor.compressed_bytes)?;
        encoder.u64(descriptor.decoded_bytes)?;
        encoder.u32(descriptor.payload_crc32c)?;
        encoder.raw(descriptor.payload_sha256.as_bytes())?;
    }
    work.checkpoint().map_err(work_error)?;
    Ok(encoder.finish())
}

fn encode_bytes(
    encoder: &mut CheckpointEncoder<'_>,
    bytes: &[u8],
    context: &str,
    work: &CheckpointWorkContext,
) -> Result<(), AppendTableError> {
    let len = u64::try_from(bytes.len())
        .map_err(|_| AppendTableError::Admission(format!("{context} length overflows u64")))?;
    encoder.u64(len)?;
    for chunk in bytes.chunks(64 * 1024) {
        encoder.raw(chunk)?;
    }
    work.checkpoint().map_err(work_error)
}

fn integrity_slices(
    slices: &[&[u8]],
    work: &CheckpointWorkContext,
) -> Result<hawdb_integrity::IntegrityDigest, AppendTableError> {
    let mut hasher = IntegrityHasher::new();
    for slice in slices {
        for chunk in slice.chunks(64 * 1024) {
            let unit = work.start_unit().map_err(work_error)?;
            hasher.update(chunk);
            unit.finish();
        }
    }
    work.checkpoint().map_err(work_error)?;
    Ok(hasher.finish())
}

fn compress(
    bytes: &[u8],
    level: i32,
    work: &CheckpointWorkContext,
) -> Result<crate::background::CheckpointBytes, AppendTableError> {
    let error = |error| {
        AppendTableError::Corruption(format!("failed to compress append segment block: {error}"))
    };
    let output = crate::compression::AdmittedCompressionOutput::new(bytes.len(), work)
        .map_err(work_error)?;
    let unit = work.start_unit().map_err(work_error)?;
    // The sink already owns the admitted complete output. A small staging
    // buffer leaves room for the native frame workspace on fixed-memory hosts.
    let mut encoder =
        crate::compression::Encoder::with_staging_capacity(output, level, 2 * 1024, work)
            .map_err(error)?;
    unit.finish();
    for chunk in bytes.chunks(64 * 1024) {
        let unit = work.start_unit().map_err(work_error)?;
        encoder.write_all(chunk).map_err(error)?;
        unit.finish();
    }
    let unit = work.start_unit().map_err(work_error)?;
    let compressed = encoder.finish().map_err(error)?.into_bytes();
    unit.finish();
    work.checkpoint().map_err(work_error)?;
    Ok(compressed)
}

#[cfg(test)]
mod tests;

fn key(
    key: &RelationalKey,
    max_key_bytes: usize,
    work: &CheckpointWorkContext,
) -> Result<crate::background::CheckpointBytes, AppendTableError> {
    work.checkpoint().map_err(work_error)?;
    if key.0.is_empty() {
        let mut encoded =
            crate::background::CheckpointBytes::zeroed(1, work).map_err(work_error)?;
        let unit = work.start_unit().map_err(work_error)?;
        encoded.as_mut_slice()[0] = GLOBAL_PARTITION_TAG;
        unit.finish();
        work.checkpoint().map_err(work_error)?;
        return Ok(encoded);
    }
    let error = |error| match error {
        crate::relational::CheckpointKeyEncodeError::Key(error) => map_relational(error),
        crate::relational::CheckpointKeyEncodeError::Work(error) => work_error(error),
    };
    let plan = crate::relational::CheckpointKeyEncoding::new(key, &[ORDERED_KEY_TAG], work)
        .map_err(error)?;
    let encoded_len = plan.encoded_len();
    if encoded_len > max_key_bytes {
        return Err(AppendTableError::Admission(format!(
            "append key contains {encoded_len} bytes, exceeding limit {max_key_bytes}"
        )));
    }
    plan.encode(work).map_err(error)
}

fn bytes_equal(
    left: &[u8],
    right: &[u8],
    work: &CheckpointWorkContext,
) -> Result<bool, AppendTableError> {
    work.checkpoint().map_err(work_error)?;
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.chunks(64 * 1024).zip(right.chunks(64 * 1024)) {
        let unit = work.start_unit().map_err(work_error)?;
        let equal = left == right;
        unit.finish();
        work.checkpoint().map_err(work_error)?;
        if !equal {
            return Ok(false);
        }
    }
    work.checkpoint().map_err(work_error)?;
    Ok(true)
}
