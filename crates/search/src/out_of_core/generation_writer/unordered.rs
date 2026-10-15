// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Governed external ordering for fixed-snapshot projection mutations.

use super::*;
use crate::build_memory::document_bytes;
use crate::build_term::Term;
use crate::out_of_core::mutation_run::SearchMutationOperation;
use crate::SearchOutOfCoreReader;
use std::num::{NonZeroU64, NonZeroUsize};

mod run;
use run::{Control, Reader, Run};

/// Private run admission. Spill/read bytes and merge visits are cumulative,
/// including duplicates and intermediate runs. The payload buffer is separate
/// from the task's shared hard memory ledger, which also owns slots/paths/I/O.
#[derive(Debug, Clone, Copy)]
pub struct SearchUnorderedMutationLimits {
    pub max_buffer_records: NonZeroUsize,
    pub max_buffer_payload_bytes: NonZeroUsize,
    pub max_input_operations: NonZeroUsize,
    pub max_spill_bytes: NonZeroU64,
    pub max_read_bytes: NonZeroU64,
    pub max_merge_visits: NonZeroUsize,
}
impl Default for SearchUnorderedMutationLimits {
    fn default() -> Self {
        Self {
            max_buffer_records: NonZeroUsize::new(1024).unwrap(),
            max_buffer_payload_bytes: NonZeroUsize::new(16 * 1024 * 1024).unwrap(),
            max_input_operations: NonZeroUsize::new(1_000_000).unwrap(),
            max_spill_bytes: NonZeroU64::new(4 * 1024 * 1024 * 1024).unwrap(),
            max_read_bytes: NonZeroU64::new(16 * 1024 * 1024 * 1024).unwrap(),
            max_merge_visits: NonZeroUsize::new(64_000_000).unwrap(),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchUnorderedMutationReport {
    pub input_operations: usize,
    pub output_operations: usize,
    pub merged_records: usize,
    pub reserved_spill_bytes: u64,
    pub reserved_read_bytes: u64,
    pub peak_buffer_payload_bytes: usize,
    pub runs_created: u64,
}

enum Buffered {
    Upsert(AdmittedDocument),
    Delete(Term),
}
impl Buffered {
    fn id(&self) -> &str {
        match self {
            Self::Upsert(doc) => &doc.id,
            Self::Delete(id) => id,
        }
    }
}

/// Sorts arbitrary input order into one invisible expected-base update.
/// All upserts must describe the same source snapshot: identical encoded
/// documents coalesce, conflicting upserts fail, and upserts win over deletes.
/// This is an artifact builder, not a graph/host batch coverage receipt.
/// Input and finalization can merge entire runs; sort limits are cumulative,
/// not resumable per-step bounds. A finite shared memory bound additionally
/// requires a task memory reservation. Caller-owned input construction happens
/// before this API's admission.
pub struct SearchOutOfCoreUnorderedMutationWriter<'a> {
    buffer: Vec<Buffered>,
    slots: QueryMemoryLease,
    levels: [Option<Run>; usize::BITS as usize],
    inner: SearchOutOfCoreMutationWriter<'a>,
    control: Control,
    payload: usize,
    failed: bool,
    _memory: QueryMemoryLease,
}
impl SearchOutOfCoreGenerationWriter {
    pub fn prepare_unordered_delta<'a>(
        reader: &'a SearchOutOfCoreReader,
        options: SearchOutOfCoreGenerationBuildOptions,
        limits: SearchUnorderedMutationLimits,
        task: RuntimeTaskContext,
    ) -> Result<SearchOutOfCoreUnorderedMutationWriter<'a>> {
        let inner = Self::prepare_streamed_delta_with_context(reader, options, task)?;
        let memory = inner.writer.memory.clone();
        let owner = memory.retained.reserve(std::mem::size_of::<
            SearchOutOfCoreUnorderedMutationWriter<'a>,
        >())?;
        let slots = memory.retained.reserve(0)?;
        let control = Control::new(
            limits,
            memory,
            inner.writer.task_context.clone(),
            reader.config().max_document_header_bytes.get(),
        );
        Ok(SearchOutOfCoreUnorderedMutationWriter {
            buffer: Vec::new(),
            slots,
            levels: std::array::from_fn(|_| None),
            inner,
            control,
            payload: 0,
            failed: false,
            _memory: owner,
        })
    }
}
impl SearchOutOfCoreUnorderedMutationWriter<'_> {
    pub fn report(&self) -> SearchUnorderedMutationReport {
        self.control.report
    }
    pub fn upsert(&mut self, document: SearchDocument) -> Result<()> {
        if self.failed {
            return Err(unusable());
        }
        self.failed = true;
        let result = (|| {
            self.control.check_input(&document.id)?;
            if document.metadata.len() > self.inner.writer.options.max_metadata_fields.get() {
                return Err(HawDBError::Storage(
                    "unordered mutation metadata fields exceed admission".into(),
                ));
            }
            let bytes = document_bytes(&document)?;
            // Incoming ownership overlaps a possible flush/merge. Keep its
            // admission alive throughout that work, rather than charging later.
            let admitted = self.inner.writer.memory.admit_document(document)?;
            self.make_room(bytes)?;
            self.push(Buffered::Upsert(admitted), bytes)
        })();
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.failed {
            return Err(unusable());
        }
        self.failed = true;
        let result = (|| {
            self.control.check_input(id)?;
            let bytes = checked_add(id.len(), Term::tracking_overhead())?;
            self.make_room(bytes)?;
            let id = Term::copy(id, Some(&self.inner.writer.memory))?;
            self.push(Buffered::Delete(id), bytes)
        })();
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    fn make_room(&mut self, bytes: usize) -> Result<()> {
        if bytes > self.control.limits.max_buffer_payload_bytes.get() {
            return Err(HawDBError::Execution(
                "unordered mutation payload cannot fit its buffer admission".into(),
            ));
        }
        if self.buffer.len() == self.control.limits.max_buffer_records.get()
            || bytes > self.control.limits.max_buffer_payload_bytes.get() - self.payload
        {
            self.flush()?;
        }
        Ok(())
    }
    fn push(&mut self, value: Buffered, bytes: usize) -> Result<()> {
        if self.buffer.len() == self.buffer.capacity() {
            let capacity = self
                .buffer
                .capacity()
                .saturating_mul(2)
                .max(1)
                .min(self.control.limits.max_buffer_records.get());
            crate::build_memory::reserve_capacity(&mut self.buffer, capacity, &mut self.slots)?;
        }
        self.buffer.push(value);
        self.payload += bytes;
        self.control.report.input_operations += 1;
        self.control.report.peak_buffer_payload_bytes = self
            .control
            .report
            .peak_buffer_payload_bytes
            .max(self.payload);
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        checkpoint(&self.inner.writer.task_context)?;
        self.buffer.sort_unstable_by(|a, b| a.id().cmp(b.id()));
        let mut output = self.control.output(&mut self.inner.writer)?;
        for entry in self.buffer.drain(..) {
            output.buffered(entry, &mut self.control, &mut self.inner.writer)?;
        }
        let mut incoming = output.finish()?;
        self.payload = 0;
        for index in 0..self.levels.len() {
            let Some(previous) = self.levels[index].take() else {
                self.levels[index] = Some(incoming);
                return Ok(());
            };
            incoming = run::merge(
                previous,
                incoming,
                &mut self.control,
                &mut self.inner.writer,
            )?;
        }
        Err(HawDBError::Storage(
            "unordered mutation run levels exhausted".into(),
        ))
    }
    pub fn prepare_publication(
        mut self,
    ) -> Result<(SearchUnorderedMutationReport, SearchOutOfCorePreparedUpdate)> {
        if self.failed {
            return Err(unusable());
        }
        self.failed = true;
        self.flush()?;
        let mut merged = None;
        for index in 0..self.levels.len() {
            if let Some(run) = self.levels[index].take() {
                merged = Some(match merged {
                    None => run,
                    Some(previous) => {
                        run::merge(previous, run, &mut self.control, &mut self.inner.writer)?
                    }
                });
            }
        }
        self.buffer = Vec::new();
        self.slots.shrink(self.slots.bytes());
        if let Some(run) = merged {
            let mut source = Reader::open(run, &mut self.control, &self.inner.writer)?;
            let mut winner = None::<run::Entry>;
            while let Some(entry) = source.next(&mut self.control)? {
                if let Some(previous) = winner.take() {
                    if previous.id() == entry.id() {
                        winner = Some(run::reduce(previous, entry, &mut self.control)?);
                        continue;
                    }
                    self.emit(previous)?;
                }
                winner = Some(entry);
            }
            if let Some(entry) = winner {
                self.emit(entry)?;
            }
            source.remove(&self.control)?;
        }
        let report = self.control.report;
        Ok((report, self.inner.prepare_publication()?))
    }
    fn emit(&mut self, entry: run::Entry) -> Result<()> {
        match entry {
            run::Entry::Delete(id) => self.inner.delete(&id)?,
            run::Entry::Upsert(record, needs_chinese) => {
                self.control.read(record.encoded_bytes)?;
                self.inner
                    .target(&record.header.id, SearchMutationOperation::Replace)?;
                self.inner
                    .writer
                    .push_spool_record(*record, needs_chinese)?;
            }
        }
        self.control.report.output_operations += 1;
        Ok(())
    }
}
fn unusable() -> HawDBError {
    HawDBError::Storage("unordered mutation writer is unusable after an input failure".into())
}

#[cfg(test)]
mod tests;
