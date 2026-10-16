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

//! Cooperative private append generation mounting.

use super::*;
use crate::projection::predicate_checkpoint::decode::MapMemory;

#[derive(Default)]
struct PartitionWatermarks {
    values: BTreeMap<RelationalKey, (usize, usize)>,
    memory: MapMemory<(usize, usize)>,
}

pub(in crate::append_table::publication) fn open(
    directory: &Path,
    expected: AppendGenerationArtifacts,
    config: AppendPublicationConfig,
    work: &CheckpointWorkContext,
) -> Result<AppendGenerationReader, AppendTableError> {
    work.checkpoint().map_err(work_error)?;
    validate_config(config)?;
    let manifest_path = directory.join(append_generation_manifest_file(expected.generation));
    let encoded = read_artifact(
        &manifest_path,
        config.max_manifest_bytes,
        "append generation manifest",
        work,
    )?;
    let digest = work.integrity(&encoded).map_err(work_error)?;
    if encoded.len() as u64 != expected.manifest_artifact.encoded_len
        || digest.crc32c.get() != expected.manifest_artifact.encoded_crc32c
        || digest.sha256 != expected.manifest_artifact.encoded_sha256
    {
        return Err(AppendTableError::Corruption(
            "append generation manifest does not match its canonical binding".to_string(),
        ));
    }
    let manifest = manifest_decode(&encoded, config, work)?;
    if manifest.generation != expected.generation
        || manifest.source_commit_epoch != expected.source_commit_epoch
        || manifest.root_set_digest != expected.root_set_digest
    {
        return Err(AppendTableError::Corruption(
            "append generation manifest does not match its canonical binding".to_string(),
        ));
    }

    let owned = crate::background::CheckpointDecodeContext {
        work: work.clone(),
        memory: std::cell::RefCell::default(),
    };
    let temporary = crate::background::CheckpointDecodeContext {
        work: work.clone(),
        memory: std::cell::RefCell::default(),
    };
    let mut readers: Vec<AppendSegmentReader> = owned
        .vector_capacity(manifest.segments.len())
        .map_err(super::super::super::binary::allocation_error)?;
    let mut watermarks: BTreeMap<String, PartitionWatermarks> = BTreeMap::new();
    let mut table_memory = MapMemory::<PartitionWatermarks>::default();
    for binding in &manifest.segments {
        let unit = work.start_unit().map_err(work_error)?;
        let path = directory.join(append_segment_file(binding.generation));
        let encoded_len = usize::try_from(binding.artifact.encoded_len).map_err(|_| {
            AppendTableError::Admission("append segment length overflows usize".to_string())
        })?;
        if encoded_len > config.segment.max_segment_bytes {
            return Err(AppendTableError::Admission(format!(
                "bound append segment contains {encoded_len} bytes, exceeding limit {}",
                config.segment.max_segment_bytes
            )));
        }
        unit.finish();
        let reader = AppendSegmentReader::open_bound_file_with_work_context(
            &path,
            binding.artifact,
            config.segment,
            work,
        )?;
        if reader.generation() != binding.generation
            || reader.source_commit_epoch() != binding.source_commit_epoch
        {
            return Err(AppendTableError::Corruption(
                "append segment identity differs from its generation binding".to_string(),
            ));
        }
        for (descriptor_index, descriptor) in reader.descriptors().iter().enumerate() {
            let unit = work.start_unit().map_err(work_error)?;
            if !manifest.schemas.contains_key(&descriptor.table) {
                return Err(AppendTableError::Corruption(format!(
                    "append segment references unknown table {}",
                    descriptor.table
                )));
            }
            let new_table = !watermarks.contains_key(&descriptor.table);
            unit.finish();
            if new_table {
                table_memory
                    .before_insert(watermarks.len(), &temporary)
                    .map_err(super::super::super::binary::allocation_error)?;
                let table = temporary
                    .string(&descriptor.table)
                    .map_err(super::super::super::binary::allocation_error)?;
                watermarks.insert(table, PartitionWatermarks::default());
            }
            let partition_watermarks = watermarks.get_mut(&descriptor.table).expect("known table");
            let unit = work.start_unit().map_err(work_error)?;
            let prior = partition_watermarks.values.get(&descriptor.partition_key);
            if prior.is_some_and(|(segment, index)| {
                let prior_reader = if *segment == readers.len() {
                    &reader
                } else {
                    &readers[*segment]
                };
                prior_reader.descriptors()[*index].max_order_key >= descriptor.min_order_key
            }) {
                return Err(AppendTableError::Corruption(format!(
                    "append segments overlap or regress table {} partition watermark",
                    descriptor.table
                )));
            }
            let new_partition = prior.is_none();
            unit.finish();
            if new_partition {
                partition_watermarks
                    .memory
                    .before_insert(partition_watermarks.values.len(), &temporary)
                    .map_err(super::super::super::binary::allocation_error)?;
                let partition = super::state::key_copy(&descriptor.partition_key, &temporary)
                    .map_err(super::super::super::binary::allocation_error)?;
                let unit = work.start_unit().map_err(work_error)?;
                partition_watermarks
                    .values
                    .insert(partition, (readers.len(), descriptor_index));
                unit.finish();
            } else {
                let unit = work.start_unit().map_err(work_error)?;
                *partition_watermarks
                    .values
                    .get_mut(&descriptor.partition_key)
                    .expect("known partition") = (readers.len(), descriptor_index);
                unit.finish();
            }
        }
        let unit = work.start_unit().map_err(work_error)?;
        readers.push(reader);
        unit.finish();
    }
    validate_generated_watermarks_by_order(
        &manifest.schemas,
        &manifest.generated_order_watermarks,
        work,
        |table| {
            watermarks
                .get(table)
                .into_iter()
                .flat_map(|partitions| partitions.values.values())
                .map(|(segment, index)| &readers[*segment].descriptors()[*index].max_order_key)
        },
    )?;
    drop(watermarks);
    drop(temporary);
    let segments = if readers.is_empty() {
        AppendSegmentRoots::Empty
    } else {
        // The decoded readers and their permits stay in the same backing array.
        let bytes =
            std::mem::size_of::<Vec<AppendSegmentReader>>() + 2 * std::mem::size_of::<usize>();
        owned
            .reserve(bytes)
            .map_err(super::super::super::binary::allocation_error)?;
        let unit = work.start_unit().map_err(work_error)?;
        let readers = Arc::new(readers);
        unit.finish();
        AppendSegmentRoots::Shared {
            readers,
            _memory: owned.memory.into_inner(),
        }
    };
    let unit = work.start_unit().map_err(work_error)?;
    let reader = AppendGenerationReader {
        manifest: manifest.into(),
        segments,
    };
    unit.finish();
    work.checkpoint().map_err(work_error)?;
    Ok(reader)
}

pub(super) fn manifest_decode(
    encoded: &[u8],
    config: AppendPublicationConfig,
    work: &CheckpointWorkContext,
) -> Result<CheckpointAppendManifest, AppendTableError> {
    work.checkpoint().map_err(work_error)?;
    if encoded.len() < MANIFEST_HEADER_BYTES || &encoded[..8] != MANIFEST_MAGIC {
        return Err(AppendTableError::Corruption(
            "append manifest header is invalid".to_string(),
        ));
    }
    let version = read_u16(encoded, 8);
    if version != MANIFEST_VERSION || read_u16(encoded, 10) != 0 || read_u32(encoded, 124) != 0 {
        return Err(AppendTableError::Corruption(format!(
            "unsupported append manifest version {version} or non-zero reserved fields"
        )));
    }
    let generation = read_u64(encoded, 12);
    let source_commit_epoch = read_u64(encoded, 20);
    let previous_raw = read_u64(encoded, 28);
    let schema_count = read_u32(encoded, 36) as usize;
    let segment_count = read_u32(encoded, 40) as usize;
    let generated_order_watermark_count = read_u32(encoded, 120) as usize;
    let payload_len = to_usize(read_u64(encoded, 44), "manifest payload length")?;
    if generation == 0
        || (source_commit_epoch == 0 && (schema_count != 0 || segment_count != 0))
        || schema_count > config.max_schemas
        || generated_order_watermark_count > config.max_schemas
        || segment_count > config.max_segments
        || MANIFEST_HEADER_BYTES.checked_add(payload_len) != Some(encoded.len())
    {
        return Err(AppendTableError::Corruption(
            "append manifest identity, counts, or payload length is invalid".to_string(),
        ));
    }
    let root_set_digest =
        Sha256Digest::from_bytes(encoded[52..84].try_into().expect("fixed root digest"));
    let payload = &encoded[MANIFEST_HEADER_BYTES..];
    if work.integrity(payload).map_err(work_error)?.sha256 != root_set_digest {
        return Err(AppendTableError::Corruption(
            "append manifest root-set digest mismatch".to_string(),
        ));
    }
    let mut hasher = hawdb_integrity::IntegrityHasher::new();
    for source in [
        &encoded[..MANIFEST_INTEGRITY_OFFSET],
        &encoded[MANIFEST_INTEGRITY_TRAILER_OFFSET..MANIFEST_HEADER_BYTES],
        payload,
    ] {
        for chunk in source.chunks(64 * 1024) {
            let unit = work.start_unit().map_err(work_error)?;
            hasher.update(chunk);
            unit.finish();
        }
    }
    let digest = hasher.finish();
    let expected_crc = read_u32(encoded, 84);
    let expected_sha: &[u8; SHA256_BYTES] = encoded[88..120].try_into().expect("fixed digest");
    if digest.crc32c.get() != expected_crc || digest.sha256.as_bytes() != expected_sha {
        return Err(AppendTableError::Corruption(
            "append manifest checksum mismatch".to_string(),
        ));
    }
    let owned = crate::background::CheckpointDecodeContext {
        work: work.clone(),
        memory: std::cell::RefCell::default(),
    };
    let mut decoder = Decoder::with_allocations(payload, Some(&owned));
    let mut schemas = BTreeMap::new();
    let mut schema_memory =
        crate::projection::predicate_checkpoint::decode::MapMemory::<AppendTableSchema>::default();
    for _ in 0..schema_count {
        let schema_record = decoder.bytes(config.max_schema_bytes, "append schema")?;
        let batch = super::super::super::codec::decode_batch(
            schema_record,
            AppendDecodeLimits::wal(),
            Some(&owned),
        )?;
        let writes_memory = owned
            .find(batch.transaction.writes.as_ptr() as usize)
            .map_err(super::super::super::binary::allocation_error)?;
        let mut writes = batch.transaction.writes.into_iter();
        let (Some(AppendWrite::CreateTable { schema }), None) = (writes.next(), writes.next())
        else {
            return Err(AppendTableError::Corruption(
                "append manifest schema record has an invalid shape".to_string(),
            ));
        };
        drop(writes);
        writes_memory.release_buffer();
        schema_memory
            .before_insert(schemas.len(), &owned)
            .map_err(super::super::super::binary::allocation_error)?;
        let name = owned
            .string(&schema.name)
            .map_err(super::super::super::binary::allocation_error)?;
        let unit = work.start_unit().map_err(work_error)?;
        if batch.epoch != 0 || schemas.insert(name, schema).is_some() {
            return Err(AppendTableError::Corruption(
                "append manifest schema record has a non-zero epoch or duplicate name".to_string(),
            ));
        }
        unit.finish();
    }
    let mut generated_order_watermarks = BTreeMap::new();
    let mut watermark_memory =
        crate::projection::predicate_checkpoint::decode::MapMemory::<i64>::default();
    for _ in 0..generated_order_watermark_count {
        watermark_memory
            .before_insert(generated_order_watermarks.len(), &owned)
            .map_err(super::super::super::binary::allocation_error)?;
        let table = decoder.string(config.max_schema_bytes, "append generated-order table")?;
        let watermark = i64::from_le_bytes(
            decoder
                .take(
                    std::mem::size_of::<i64>(),
                    "append generated-order watermark",
                )?
                .try_into()
                .expect("fixed i64"),
        );
        let unit = work.start_unit().map_err(work_error)?;
        if generated_order_watermarks
            .insert(table, watermark)
            .is_some()
        {
            return Err(AppendTableError::Corruption(
                "append manifest contains duplicate generated-order watermarks".to_string(),
            ));
        }
        unit.finish();
    }
    let mut segments = decoder.vector(segment_count)?;
    for _ in 0..segment_count {
        let binding = AppendSegmentBinding {
            generation: decoder.u64()?,
            source_commit_epoch: decoder.u64()?,
            artifact: decode_artifact(&mut decoder)?,
        };
        let unit = work.start_unit().map_err(work_error)?;
        segments.push(binding);
        unit.finish();
    }
    decoder.finish("append manifest")?;
    for pair in segments.windows(2) {
        let unit = work.start_unit().map_err(work_error)?;
        let ordered = pair[0].generation < pair[1].generation
            && pair[0].source_commit_epoch <= pair[1].source_commit_epoch;
        if !ordered {
            return Err(AppendTableError::Corruption(
                "append manifest segment bindings are unordered or newer than the generation"
                    .to_string(),
            ));
        }
        unit.finish();
    }
    if segments.last().is_some_and(|binding| {
        binding.generation > generation || binding.source_commit_epoch > source_commit_epoch
    }) {
        return Err(AppendTableError::Corruption(
            "append manifest segment bindings are unordered or newer than the generation"
                .to_string(),
        ));
    }
    let previous_generation = (previous_raw != 0).then_some(previous_raw);
    if previous_generation.is_some_and(|previous| previous >= generation) {
        return Err(AppendTableError::Corruption(
            "append manifest previous generation does not precede the current generation"
                .to_string(),
        ));
    }
    work.checkpoint().map_err(work_error)?;
    if !schemas.is_empty() || !segments.is_empty() || !generated_order_watermarks.is_empty() {
        owned
            .reserve(
                std::mem::size_of::<CheckpointAppendManifest>() + 2 * std::mem::size_of::<usize>(),
            )
            .map_err(super::super::super::binary::allocation_error)?;
    }
    Ok(CheckpointAppendManifest {
        data: AppendGenerationManifest {
            generation,
            source_commit_epoch,
            previous_generation,
            root_set_digest,
            schemas,
            generated_order_watermarks,
            segments,
        },
        memory: owned.memory.into_inner(),
    })
}

fn read_artifact(
    path: &Path,
    max_bytes: usize,
    context: &str,
    work: &CheckpointWorkContext,
) -> Result<crate::background::CheckpointBytes, AppendTableError> {
    let unit = work.start_unit().map_err(work_error)?;
    let wave = work.io_wave().map_err(work_error)?;
    let file = File::open(path).map_err(durability("open append artifact"))?;
    let encoded_len = file
        .metadata()
        .map_err(durability("read append artifact metadata"))?
        .len();
    if encoded_len > max_bytes as u64 {
        return Err(AppendTableError::Admission(format!(
            "{context} contains {encoded_len} bytes, exceeding limit {max_bytes}"
        )));
    }
    let mut reader = file.take((max_bytes as u64).saturating_add(1));
    drop(wave);
    unit.finish();
    let encoded_len = to_usize(encoded_len, "artifact length")?;
    let mut encoded =
        crate::background::CheckpointBytes::new(encoded_len, work).map_err(work_error)?;
    let mut buffer = [0; 64 * 1024];
    loop {
        let unit = work.start_unit().map_err(work_error)?;
        let wave = work.io_wave().map_err(work_error)?;
        let count = reader
            .read(&mut buffer)
            .map_err(durability("read append artifact"))?;
        drop(wave);
        unit.finish();
        if count > max_bytes.saturating_sub(encoded.len()) {
            return Err(AppendTableError::Admission(format!(
                "{context} exceeds limit {max_bytes}"
            )));
        }
        encoded.append(&buffer[..count], work).map_err(work_error)?;
        if count == 0 {
            break;
        }
    }
    work.checkpoint().map_err(work_error)?;
    Ok(encoded)
}
