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

//! Private append segment mount using admitted I/O and descriptor units.

use super::*;

pub(in crate::append_table::segment) fn open(
    path: &Path,
    expected: AppendSegmentArtifactMetadata,
    config: AppendSegmentConfig,
    work: &CheckpointWorkContext,
) -> Result<AppendSegmentReader, AppendTableError> {
    work.checkpoint().map_err(work_error)?;
    validate_config(config)?;
    let unit = work.start_unit().map_err(work_error)?;
    let wave = work.io_wave().map_err(work_error)?;
    let mut file = File::open(path)
        .map_err(|error| AppendTableError::from_io("open append segment", error))?;
    let encoded_len = usize::try_from(
        file.metadata()
            .map_err(|error| AppendTableError::from_io("read append segment metadata", error))?
            .len(),
    )
    .map_err(|_| {
        AppendTableError::Admission("append segment length overflows usize".to_string())
    })?;
    if encoded_len as u64 != expected.encoded_len {
        return Err(AppendTableError::Corruption(
            "append segment length does not match its canonical binding".to_string(),
        ));
    }
    let mut header_bytes = [0; SEGMENT_HEADER_BYTES];
    file.read_exact(&mut header_bytes)
        .map_err(|error| AppendTableError::from_io("read append segment header", error))?;
    let header = decode_segment_header(&header_bytes, encoded_len, config)?;
    drop(wave);
    unit.finish();

    let mut artifact_hasher = IntegrityHasher::new();
    artifact_hasher.update(&header_bytes);
    let mut body_hasher = IntegrityHasher::new();
    let mut remaining = encoded_len - SEGMENT_HEADER_BYTES;
    let mut buffer = crate::background::CheckpointBytes::zeroed(remaining.min(16 * 1024), work)
        .map_err(work_error)?;
    while remaining > 0 {
        let chunk_len = remaining.min(buffer.len());
        let unit = work.start_unit().map_err(work_error)?;
        let wave = work.io_wave().map_err(work_error)?;
        file.read_exact(&mut buffer.as_mut_slice()[..chunk_len])
            .map_err(|error| AppendTableError::from_io("stream append segment", error))?;
        artifact_hasher.update(&buffer[..chunk_len]);
        body_hasher.update(&buffer[..chunk_len]);
        remaining -= chunk_len;
        drop(wave);
        unit.finish();
    }
    let artifact_digest = artifact_hasher.finish();
    if artifact_digest.crc32c.get() != expected.encoded_crc32c
        || artifact_digest.sha256 != expected.encoded_sha256
    {
        return Err(AppendTableError::Corruption(
            "append segment does not match its canonical binding".to_string(),
        ));
    }
    let body_digest = body_hasher.finish();
    if body_digest.crc32c.get() != read_u32(&header_bytes, 52)
        || body_digest.sha256.as_bytes()
            != <&[u8; SHA256_BYTES]>::try_from(&header_bytes[56..88]).expect("fixed SHA-256")
    {
        return Err(AppendTableError::Corruption(
            "append segment checksum mismatch".to_string(),
        ));
    }

    drop(buffer);
    let unit = work.start_unit().map_err(work_error)?;
    let wave = work.io_wave().map_err(work_error)?;
    file.seek(SeekFrom::Start(SEGMENT_HEADER_BYTES as u64))
        .map_err(|error| AppendTableError::from_io("seek append segment directory", error))?;
    drop(wave);
    unit.finish();
    let mut directory =
        crate::background::CheckpointBytes::new(header.directory_len, work).map_err(work_error)?;
    let mut buffer = [0; 64 * 1024];
    while directory.len() < header.directory_len {
        let chunk_len = (header.directory_len - directory.len()).min(buffer.len());
        let unit = work.start_unit().map_err(work_error)?;
        let wave = work.io_wave().map_err(work_error)?;
        file.read_exact(&mut buffer[..chunk_len])
            .map_err(|error| AppendTableError::from_io("read append segment directory", error))?;
        drop(wave);
        unit.finish();
        directory
            .append(&buffer[..chunk_len], work)
            .map_err(work_error)?;
    }
    let owned = crate::background::CheckpointDecodeContext {
        work: work.clone(),
        memory: std::cell::RefCell::default(),
    };
    let descriptors = decode_directory_with_allocations(
        &directory,
        header.descriptor_count,
        header.payload_len,
        config,
        Some(&owned),
    )?;
    drop(directory);
    // Share the admitted Vec backing allocation without copying the complete
    // directory into a new Arc slice. Its permit remains in the reader inventory.
    let bytes =
        std::mem::size_of::<Vec<AppendSegmentBlockDescriptor>>() + 2 * std::mem::size_of::<usize>();
    owned
        .reserve(bytes)
        .map_err(super::super::super::binary::allocation_error)?;
    for bytes in [
        std::mem::size_of::<AppendSegmentFileSource>() + 2 * std::mem::size_of::<usize>(),
        std::mem::size_of::<Mutex<AppendDecodedBlockCache>>() + 2 * std::mem::size_of::<usize>(),
        std::mem::size_of::<Mutex<AppendReadResultCache>>() + 2 * std::mem::size_of::<usize>(),
    ] {
        owned
            .reserve(bytes)
            .map_err(super::super::super::binary::allocation_error)?;
    }
    let unit = work.start_unit().map_err(work_error)?;
    let descriptors = Arc::new(descriptors);
    unit.finish();
    let unit = work.start_unit().map_err(work_error)?;
    let reader = AppendSegmentReader {
        generation: header.generation,
        source_commit_epoch: header.source_commit_epoch,
        source: AppendSegmentSource::File(Arc::new(AppendSegmentFileSource {
            file: Mutex::new(file),
            encoded_len,
        })),
        payload_start: SEGMENT_HEADER_BYTES + header.directory_len,
        descriptors,
        config,
        decoded_block_cache: Arc::new(Mutex::new(AppendDecodedBlockCache::new(
            config.decoded_block_cache_bytes / 2,
        ))),
        read_result_cache: Arc::new(Mutex::new(AppendReadResultCache::new(
            config.decoded_block_cache_bytes - config.decoded_block_cache_bytes / 2,
        ))),
        _memory: owned.memory.into_inner(),
    };
    unit.finish();
    work.checkpoint().map_err(work_error)?;
    Ok(reader)
}
