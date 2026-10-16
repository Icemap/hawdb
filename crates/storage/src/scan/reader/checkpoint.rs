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

//! Private checkpoint reads use the captured file identity and bounded waves.
//! They bypass serving caches and retain admitted output/scratch ownership.

use super::*;
use crate::background::{CheckpointBytes, CheckpointWorkContext, CheckpointWorkError};
use crate::immutable_files::CheckpointImmutableFileError;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(crate) enum CheckpointRangeReadError {
    Read(SegmentReadError),
    Work(CheckpointWorkError),
}

impl From<SegmentReadError> for CheckpointRangeReadError {
    fn from(error: SegmentReadError) -> Self {
        Self::Read(error)
    }
}

impl From<CheckpointWorkError> for CheckpointRangeReadError {
    fn from(error: CheckpointWorkError) -> Self {
        Self::Work(error)
    }
}

impl Display for CheckpointRangeReadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => Display::fmt(error, formatter),
            Self::Work(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for CheckpointRangeReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Work(error) => Some(error),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RangeIdentity {
    artifact_id: u64,
    segment_id: u64,
    offset: u64,
    length: u64,
    digest: Option<crate::cache::ContentDigest>,
}

impl RangeIdentity {
    fn of(range: &SegmentReadRange) -> Self {
        Self {
            artifact_id: range.artifact_id,
            segment_id: range.segment_ids.first().copied().unwrap_or_default(),
            offset: range.offset,
            length: range.length.get(),
            digest: range.content_digest,
        }
    }
}

pub(crate) struct CheckpointRangeReadProgress {
    identity: RangeIdentity,
    artifact: Arc<RegisteredArtifact>,
    registered: bool,
    opened: bool,
    shared: Option<Arc<File>>,
    payload: Option<CheckpointBytes>,
    scratch: Option<CheckpointBytes>,
    pending: Option<usize>,
    crc: hawdb_integrity::Crc32cHasher,
    checked: usize,
}

impl FileSegmentRangeReader {
    pub(crate) fn checkpoint_range_with_work_context(
        &self,
        range: &SegmentReadRange,
        work: &CheckpointWorkContext,
    ) -> Result<CheckpointBytes, CheckpointRangeReadError> {
        self.checkpoint_range_with_preparation(range, &mut None, work)
    }

    pub(crate) fn checkpoint_range_with_preparation(
        &self,
        range: &SegmentReadRange,
        pending: &mut Option<CheckpointRangeReadProgress>,
        work: &CheckpointWorkContext,
    ) -> Result<CheckpointBytes, CheckpointRangeReadError> {
        work.checkpoint()?;
        let artifact =
            self.artifacts
                .get(&range.artifact_id)
                .ok_or(SegmentReadError::ArtifactNotFound {
                    artifact_id: range.artifact_id,
                })?;
        let identity = RangeIdentity::of(range);
        if let Some(state) = pending.as_ref()
            && (state.identity != identity || !Arc::ptr_eq(artifact, &state.artifact))
        {
            return Err(range_io_error(
                range,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "retained segment range source changed",
                ),
            )
            .into());
        }
        let length =
            usize::try_from(range.length.get()).map_err(|_| SegmentReadError::RangeTooLarge {
                artifact_id: range.artifact_id,
                length: range.length.get(),
            })?;
        let state = pending.get_or_insert_with(|| CheckpointRangeReadProgress {
            identity,
            artifact: artifact.clone(),
            registered: false,
            opened: false,
            shared: None,
            payload: None,
            scratch: None,
            pending: None,
            crc: hawdb_integrity::Crc32cHasher::new(),
            checked: 0,
        });
        state.read(range, length, work)?;
        if let Some(expected) = range.content_digest
            && crate::cache::ContentDigest(state.crc.finish()) != expected
        {
            if let Some(cache) = &self.cache {
                cache.record_digest_mismatch();
            }
            return Err(SegmentReadError::DigestMismatch {
                artifact_id: range.artifact_id,
                segment_id: range.segment_ids.first().copied().unwrap_or_default(),
            }
            .into());
        }
        work.checkpoint()?;
        let mut finished = pending.take().expect("completed range state");
        Ok(finished.payload.take().expect("completed range payload"))
    }
}

impl CheckpointRangeReadProgress {
    fn read(
        &mut self,
        range: &SegmentReadRange,
        length: usize,
        work: &CheckpointWorkContext,
    ) -> Result<(), CheckpointRangeReadError> {
        if !self.registered {
            let unit = work.start_unit()?;
            self.artifact
                .registration()
                .map_err(|source| checkpoint_range_io_error(range, source, work))?;
            self.registered = true;
            unit.finish();
        }
        let registration = self
            .artifact
            .registration
            .get()
            .expect("captured registration");
        if !self.opened {
            self.shared = match registration.binding.as_ref() {
                Some(binding) => Some(
                    registration
                        .handles
                        .checkpoint_get(binding, &registration.context, work)
                        .map_err(|error| match error {
                            CheckpointImmutableFileError::Io(source) => {
                                CheckpointRangeReadError::Read(range_io_error(range, source))
                            }
                            CheckpointImmutableFileError::Work(error) => {
                                CheckpointRangeReadError::Work(error)
                            }
                        })?,
                ),
                None => {
                    let unit = work.start_unit()?;
                    if self.artifact.file.get().is_none() {
                        let _wave = work.io_wave()?;
                        let opened = crate::file_io::OpenOptions::new()
                            .read(true)
                            .open_with_context(&self.artifact.path, &registration.context)
                            .map_err(|source| checkpoint_range_io_error(range, source, work))?;
                        let _ = self.artifact.file.set(opened);
                    }
                    self.opened = true;
                    unit.finish();
                    None
                }
            };
            self.opened = true;
        }
        let file = self.shared.as_ref().map_or_else(
            || self.artifact.file.get().expect("captured opened artifact"),
            |file| file.as_ref(),
        );
        if self.payload.is_none() {
            self.payload = Some(CheckpointBytes::new_retained(length, work)?);
        }
        if self.scratch.is_none() {
            self.scratch = Some(CheckpointBytes::new_retained(length.min(64 * 1024), work)?);
        }
        let scratch = self.scratch.as_mut().expect("admitted scratch");
        scratch.extend_zeroes_to(length.min(64 * 1024), work)?;
        let payload = self.payload.as_mut().expect("admitted payload");
        while payload.len() < length {
            if self.pending.is_none() {
                let start = payload.len();
                let bytes = length.saturating_sub(start).min(64 * 1024);
                let unit = work.start_unit()?;
                let _wave = work.io_wave()?;
                let offset = range.offset.checked_add(start as u64).ok_or_else(|| {
                    range_io_error(
                        range,
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "segment range offset overflows u64",
                        ),
                    )
                })?;
                read_exact_at(file, &mut scratch.as_mut_slice()[..bytes], offset)
                    .map_err(|source| checkpoint_range_io_error(range, source, work))?;
                self.pending = Some(bytes);
                unit.finish();
            }
            let bytes = self.pending.expect("complete physical block");
            payload.append_block_retained(&scratch[..bytes], work)?;
            self.pending = None;
        }
        if range.content_digest.is_some() {
            while self.checked < payload.len() {
                let unit = work.start_unit()?;
                let end = self.checked.saturating_add(64 * 1024).min(payload.len());
                self.crc.update(&payload[self.checked..end]);
                self.checked = end;
                unit.finish();
            }
        }
        work.checkpoint()?;
        Ok(())
    }
}

fn checkpoint_range_io_error(
    range: &SegmentReadRange,
    source: std::io::Error,
    work: &CheckpointWorkContext,
) -> CheckpointRangeReadError {
    match hawdb_core::error::file_descriptor_error(&source) {
        Some(error) => CheckpointRangeReadError::Work(
            work.record_failure(CheckpointWorkError::FileDescriptors(error)),
        ),
        None => CheckpointRangeReadError::Read(range_io_error(range, source)),
    }
}
