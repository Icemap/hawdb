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

//! Keep file progress across work denial and release each completed payload.
use super::*;
use hawdb_integrity::Crc32cHasher;

pub(super) struct Progress {
    // Close handles before the temporary-path guards remove their files.
    payload_file: Option<File>,
    descriptor_file: Option<File>,
    payload_temporary: TemporarySourceScanPath,
    descriptor_temporary: TemporarySourceScanPath,
    path: PathBuf,
    graph_epoch: u64,
    segment_count: usize,
    segment: usize,
    payload_length: u64,
    segment_written: usize,
    segment_crc: Crc32cHasher,
    payload_synced: bool,
    payload_published: bool,
    descriptor: Option<CheckpointText>,
    descriptor_part: usize,
    descriptor_part_written: usize,
    descriptor_crc: Crc32cHasher,
    descriptor_checksum: Option<u64>,
    descriptor_written: u64,
    descriptor_synced: bool,
    descriptor_published: bool,
    publication: Option<SourceScanPublication>,
}

impl Progress {
    fn new(path: &Path, projection: &SourceScanProjection) -> Self {
        Self {
            payload_file: None,
            descriptor_file: None,
            payload_temporary: TemporarySourceScanPath(None),
            descriptor_temporary: TemporarySourceScanPath(None),
            path: path.to_owned(),
            graph_epoch: projection.graph_epoch,
            segment_count: projection.segments.len(),
            segment: 0,
            payload_length: 0,
            segment_written: 0,
            segment_crc: Crc32cHasher::new(),
            payload_synced: false,
            payload_published: false,
            descriptor: None,
            descriptor_part: 0,
            descriptor_part_written: 0,
            descriptor_crc: Crc32cHasher::new(),
            descriptor_checksum: None,
            descriptor_written: 0,
            descriptor_synced: false,
            descriptor_published: false,
            publication: None,
        }
    }

    fn advance(
        &mut self,
        projection: &mut SourceScanProjection,
        preparation: &mut SourceScanWritePreparation,
        work: &CheckpointWorkContext,
    ) -> Result<SourceScanPublication> {
        if let Some(publication) = self.publication {
            return Ok(publication);
        }
        let payload_path = self.path.join(SOURCE_SCAN_PAYLOAD_FILE);
        let payload_tmp = payload_path.with_extension("hawdb.tmp");
        if !self.payload_published {
            if self.payload_file.is_none() && !self.payload_synced {
                self.payload_file = Some(create(&payload_tmp, work)?);
                self.payload_temporary.0 = Some(payload_tmp.clone());
            }
            while self.segment < self.segment_count {
                preparation.prepare_segment(projection, self.segment, work)?;
                let payload = preparation.segments[self.segment]
                    .payload
                    .as_ref()
                    .expect("prepared current source payload");
                let length = NonZeroU64::new(payload.len() as u64).ok_or_else(|| {
                    HawDBError::Storage("source scan segment payload is empty".into())
                })?;
                let end = self
                    .payload_length
                    .checked_add(length.get())
                    .ok_or_else(|| {
                        HawDBError::Storage("source scan payload artifact length overflow".into())
                    })?;
                while self.segment_written < payload.len() {
                    let next = (self.segment_written + 64 * 1024).min(payload.len());
                    let block = &payload[self.segment_written..next];
                    let unit = work.start_unit().map_err(HawDBError::from_storage_error)?;
                    let _wave = work.io_wave().map_err(HawDBError::from_storage_error)?;
                    write_block(
                        self.payload_file.as_mut().expect("owned payload file"),
                        self.payload_length + self.segment_written as u64,
                        block,
                    )?;
                    self.segment_crc.update(block);
                    self.segment_written = next;
                    unit.finish();
                }
                projection.segments.make_mut(work)?[self.segment].payload_range =
                    Some(SegmentPayloadRange {
                        artifact_id: SOURCE_SCAN_ARTIFACT_ID,
                        offset: self.payload_length,
                        length,
                        checksum: self.segment_crc.finish(),
                    });
                // The range and complete file prefix are owned before releasing
                // the encoded segment. A fresh attempt starts at the next one.
                preparation.segments[self.segment] = SegmentEncoding::default();
                self.segment += 1;
                self.payload_length = end;
                self.segment_written = 0;
                self.segment_crc = Crc32cHasher::new();
                work.checkpoint().map_err(HawDBError::from_storage_error)?;
            }
            if !self.payload_synced {
                synchronize(
                    self.payload_file.as_ref().expect("owned payload file"),
                    work,
                )?;
                self.payload_synced = true;
            }
            self.payload_file = None;
            publish(&payload_tmp, &payload_path, work)?;
            self.payload_temporary.0 = None;
            self.payload_published = true;
        }
        let descriptor_path = self.path.join(SOURCE_SCAN_DESCRIPTOR_FILE);
        let descriptor_tmp = descriptor_path.with_extension("hawdb.tmp");
        if self.descriptor_file.is_none() && !self.descriptor_synced {
            self.descriptor_file = Some(create(&descriptor_tmp, work)?);
            self.descriptor_temporary.0 = Some(descriptor_tmp.clone());
        }
        while self.descriptor_part <= self.segment_count + 1 {
            let footer = self.descriptor_part == self.segment_count + 1;
            if self.descriptor.is_none() {
                let mut body = CheckpointText::new();
                if self.descriptor_part == 0 {
                    encode_descriptor_header(self.graph_epoch, &mut body, work)?;
                } else if footer {
                    let checksum = *self
                        .descriptor_checksum
                        .get_or_insert_with(|| self.descriptor_crc.finish());
                    body.fields(format_args!("checksum\t{checksum}\n"), work)?;
                } else {
                    encode_descriptor_segment(
                        &projection.segments[self.descriptor_part - 1],
                        &mut body,
                        work,
                    )?;
                }
                self.descriptor = Some(body);
            }
            let descriptor = self.descriptor.as_ref().expect("encoded descriptor part");
            while self.descriptor_part_written < descriptor.len() {
                let next = (self.descriptor_part_written + 64 * 1024).min(descriptor.len());
                let block = &descriptor.as_bytes()[self.descriptor_part_written..next];
                let unit = work.start_unit().map_err(HawDBError::from_storage_error)?;
                let _wave = work.io_wave().map_err(HawDBError::from_storage_error)?;
                let end = self
                    .descriptor_written
                    .checked_add(block.len() as u64)
                    .ok_or_else(|| {
                        HawDBError::Storage("Source descriptor length overflow".into())
                    })?;
                write_block(
                    self.descriptor_file
                        .as_mut()
                        .expect("owned descriptor file"),
                    self.descriptor_written,
                    block,
                )?;
                if !footer {
                    self.descriptor_crc.update(block);
                }
                self.descriptor_written = end;
                self.descriptor_part_written = next;
                unit.finish();
            }
            self.descriptor_part += 1;
            self.descriptor_part_written = 0;
            self.descriptor = None;
            work.checkpoint().map_err(HawDBError::from_storage_error)?;
        }
        let checksum = self
            .descriptor_checksum
            .expect("complete descriptor checksum");
        if !self.descriptor_synced {
            synchronize(
                self.descriptor_file
                    .as_ref()
                    .expect("owned descriptor file"),
                work,
            )?;
            self.descriptor_synced = true;
        }
        self.descriptor_file = None;
        if !self.descriptor_published {
            publish(&descriptor_tmp, &descriptor_path, work)?;
            self.descriptor_temporary.0 = None;
            self.descriptor_published = true;
        }
        let publication = SourceScanPublication {
            graph_epoch: self.graph_epoch,
            descriptor_checksum: checksum,
        };
        self.publication = Some(publication);
        self.descriptor = None;
        work.checkpoint().map_err(HawDBError::from_storage_error)?;
        Ok(publication)
    }
}

pub(super) fn advance(
    path: &Path,
    projection: &mut SourceScanProjection,
    preparation: &mut SourceScanWritePreparation,
    work: &CheckpointWorkContext,
) -> Result<SourceScanPublication> {
    let mut progress = preparation
        .publication
        .take()
        .unwrap_or_else(|| Progress::new(path, projection));
    let result = if progress.path != path
        || progress.graph_epoch != projection.graph_epoch
        || progress.segment_count != projection.segments.len()
    {
        Err(HawDBError::StorageIntegrity(
            "Source scan preparation cannot change its captured output".into(),
        ))
    } else {
        progress.advance(projection, preparation, work)
    };
    preparation.publication = Some(progress);
    result
}

fn create(path: &Path, work: &CheckpointWorkContext) -> Result<File> {
    let unit = work.start_unit().map_err(HawDBError::from_storage_error)?;
    let _wave = work.io_wave().map_err(HawDBError::from_storage_error)?;
    let file = File::options().write(true).create_new(true).open(path)?;
    unit.finish();
    Ok(file)
}

fn synchronize(file: &File, work: &CheckpointWorkContext) -> Result<()> {
    let unit = work.start_unit().map_err(HawDBError::from_storage_error)?;
    let _wave = work.io_wave().map_err(HawDBError::from_storage_error)?;
    file.sync_all()?;
    unit.finish();
    Ok(())
}

fn publish(temporary: &Path, destination: &Path, work: &CheckpointWorkContext) -> Result<()> {
    let unit = work.start_unit().map_err(HawDBError::from_storage_error)?;
    let _wave = work.io_wave().map_err(HawDBError::from_storage_error)?;
    durable_replace_file(temporary, destination)?;
    unit.finish();
    Ok(())
}

fn write_block(file: &mut File, position: u64, block: &[u8]) -> Result<()> {
    if let Err(error) = file.write_all(block) {
        if let Err(rollback) = file
            .set_len(position)
            .and_then(|()| file.seek(SeekFrom::Start(position)).map(|_| ()))
        {
            return Err(HawDBError::StorageIntegrity(format!(
                "Source scan write failed ({error}); prefix rollback failed ({rollback})"
            )));
        }
        return Err(error.into());
    }
    Ok(())
}
