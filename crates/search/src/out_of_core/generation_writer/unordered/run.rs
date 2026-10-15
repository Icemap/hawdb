// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use crate::build_control::CheckedWriter;
use crate::build_memory::path::OwnedPath;
use crate::lexical_projection::DocumentsDigest;
use hawdb_integrity::Crc32cHasher;
use std::io::{BufWriter, Write};

pub(super) struct Control {
    pub(super) limits: SearchUnorderedMutationLimits,
    pub(super) report: SearchUnorderedMutationReport,
    memory: BuildMemory,
    task: RuntimeTaskContext,
    max_id_bytes: usize,
}
impl Control {
    pub(super) fn new(
        limits: SearchUnorderedMutationLimits,
        memory: BuildMemory,
        task: RuntimeTaskContext,
        max_id_bytes: usize,
    ) -> Self {
        Self {
            limits,
            report: Default::default(),
            memory,
            task,
            max_id_bytes,
        }
    }
    pub(super) fn check_input(&self, id: &str) -> Result<()> {
        checkpoint(&self.task)?;
        validate_id(id, self.max_id_bytes)?;
        if self.report.input_operations >= self.limits.max_input_operations.get() {
            return Err(invalid("input operation admission exhausted"));
        }
        Ok(())
    }
    pub(super) fn read(&mut self, bytes: u64) -> Result<()> {
        checkpoint(&self.task)?;
        let next = self
            .report
            .reserved_read_bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.max_read_bytes.get())
            .ok_or_else(|| invalid("read byte admission exhausted"))?;
        self.report.reserved_read_bytes = next;
        Ok(())
    }
    fn write(&mut self, writer: &mut SearchOutOfCoreGenerationWriter, bytes: u64) -> Result<()> {
        checkpoint(&self.task)?;
        let next = self
            .report
            .reserved_spill_bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.max_spill_bytes.get())
            .ok_or_else(|| invalid("spill byte admission exhausted"))?;
        writer.stage.reserve_additional_disk(bytes)?;
        self.report.reserved_spill_bytes = next;
        Ok(())
    }
    fn visit(&mut self) -> Result<()> {
        checkpoint(&self.task)?;
        if self.report.merged_records >= self.limits.max_merge_visits.get() {
            return Err(invalid("merge visit admission exhausted"));
        }
        self.report.merged_records += 1;
        Ok(())
    }
    pub(super) fn output(
        &mut self,
        writer: &mut SearchOutOfCoreGenerationWriter,
    ) -> Result<Output> {
        let sequence = self.report.runs_created;
        self.report.runs_created = sequence
            .checked_add(1)
            .ok_or_else(|| invalid("run identity overflow"))?;
        let _name = self.memory.retained.reserve(64)?;
        let name = format!("unordered-{sequence}.run");
        if name.capacity() > 64 {
            return Err(invalid("run name exceeds admission"));
        }
        let io = io::GenerationIo::new(&self.memory, &self.task);
        let path = io.path(&writer.stage.path, Path::new(&name))?;
        let buffer = self.memory.spool.reserve(SPOOL_BUFFER_BYTES)?;
        let file = io.native(&[&path], || {
            OpenOptions::new().create_new(true).write(true).open(&*path)
        })??;
        let mut file = BufWriter::with_capacity(SPOOL_BUFFER_BYTES, file);
        if file.capacity() != SPOOL_BUFFER_BYTES {
            return Err(invalid("run buffer exceeds admission"));
        }
        self.write(writer, spool::SPOOL_HEADER.len() as u64)?;
        file.write_all(spool::SPOOL_HEADER)?;
        Ok(Output {
            file,
            path,
            records: 0,
            bytes: spool::SPOOL_HEADER.len() as u64,
            digest: Default::default(),
            _buffer: buffer,
        })
    }
}
fn invalid(message: &str) -> HawDBError {
    HawDBError::Storage(format!("unordered mutation {message}"))
}
fn validate_id(id: &str, max: usize) -> Result<()> {
    if id.is_empty() || id.len() > max {
        Err(invalid("ID is empty or exceeds admission"))
    } else {
        Ok(())
    }
}
fn evidence(digest: &mut DocumentsDigest, record: u64, bytes: u64, tag: u64) {
    digest.add_record(record ^ (tag << 56), bytes);
}

pub(super) struct Run {
    path: OwnedPath,
    records: usize,
    bytes: u64,
    digest: DocumentsDigest,
}
pub(super) struct Output {
    file: BufWriter<File>,
    path: OwnedPath,
    records: usize,
    bytes: u64,
    digest: DocumentsDigest,
    _buffer: QueryMemoryLease,
}
impl Output {
    fn reserve(
        &mut self,
        bytes: u64,
        control: &mut Control,
        writer: &mut SearchOutOfCoreGenerationWriter,
    ) -> Result<()> {
        control.write(writer, bytes)?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| invalid("run extent overflow"))?;
        self.records = self
            .records
            .checked_add(1)
            .ok_or_else(|| invalid("run record count overflow"))?;
        Ok(())
    }
    pub(super) fn buffered(
        &mut self,
        entry: Buffered,
        control: &mut Control,
        writer: &mut SearchOutOfCoreGenerationWriter,
    ) -> Result<()> {
        match entry {
            Buffered::Delete(id) => self.delete(&id, control, writer),
            Buffered::Upsert(document) => {
                let encoding = DocumentEncoding::new_with_context(&document, Some(&control.task))?;
                let bytes = encoding.len() as u64;
                if bytes > writer.options.max_record_bytes.get() {
                    return Err(invalid("record exceeds encoded admission"));
                }
                let chinese = crate::analyzer_workspace::document_needs_workspace(&document);
                let extent = bytes
                    .checked_add(18)
                    .ok_or_else(|| invalid("record extent overflow"))?;
                self.reserve(extent, control, writer)?;
                self.file.write_all(&[1, u8::from(chinese)])?;
                let mut record = DocumentsDigest::default();
                spool::write_frame_with_context(
                    &mut self.file,
                    &encoding,
                    &mut record,
                    &control.memory,
                    &control.task,
                )?;
                evidence(
                    &mut self.digest,
                    record.finish(),
                    extent,
                    2 + u64::from(chinese),
                );
                Ok(())
            }
        }
    }
    fn delete(
        &mut self,
        id: &str,
        control: &mut Control,
        writer: &mut SearchOutOfCoreGenerationWriter,
    ) -> Result<()> {
        let length = id.len() as u64;
        let extent = length
            .checked_add(17)
            .ok_or_else(|| invalid("delete extent overflow"))?;
        self.reserve(extent, control, writer)?;
        let mut digest = Crc32cHasher::new();
        digest.update(id.as_bytes());
        let checksum = digest.finish();
        let mut output = CheckedWriter::new(&mut self.file, Some(&control.task));
        output.write_all(&[0])?;
        output.write_all(&length.to_le_bytes())?;
        output.write_all(id.as_bytes())?;
        output.write_all(&checksum.to_le_bytes())?;
        let mut record = DocumentsDigest::default();
        record.add_record(checksum, length);
        evidence(&mut self.digest, record.finish(), extent, 0);
        Ok(())
    }
    fn copied(
        &mut self,
        entry: &Entry,
        control: &mut Control,
        writer: &mut SearchOutOfCoreGenerationWriter,
    ) -> Result<()> {
        match entry {
            Entry::Delete(id) => self.delete(id, control, writer),
            Entry::Upsert(record, chinese) => {
                let extent = record
                    .encoded_bytes
                    .checked_add(18)
                    .ok_or_else(|| invalid("record extent overflow"))?;
                control.read(record.encoded_bytes)?;
                self.reserve(extent, control, writer)?;
                self.file.write_all(&[1, u8::from(*chinese)])?;
                self.file.write_all(&record.encoded_bytes.to_le_bytes())?;
                self.file.write_all(&record.checksum().to_le_bytes())?;
                record.write_to(&mut self.file, &control.memory, &control.task)?;
                let mut value = DocumentsDigest::default();
                record.add_digest(&mut value);
                evidence(
                    &mut self.digest,
                    value.finish(),
                    extent,
                    2 + u64::from(*chinese),
                );
                Ok(())
            }
        }
    }
    pub(super) fn finish(mut self) -> Result<Run> {
        self.file.flush()?;
        Ok(Run {
            path: self.path,
            records: self.records,
            bytes: self.bytes,
            digest: self.digest,
        })
    }
}

pub(super) enum Entry {
    Delete(Term),
    Upsert(Box<spool::SpoolRecord>, bool),
}
impl Entry {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Delete(id) => id,
            Self::Upsert(record, _) => &record.header.id,
        }
    }
}
pub(super) struct Reader {
    cursor: spool::SpoolCursor,
    run: Run,
    position: u64,
    records: usize,
    digest: DocumentsDigest,
    previous: Option<Term>,
    memory: BuildMemory,
    task: RuntimeTaskContext,
    _memory: QueryMemoryLease,
}
impl Reader {
    pub(super) fn open(
        run: Run,
        control: &mut Control,
        writer: &SearchOutOfCoreGenerationWriter,
    ) -> Result<Self> {
        control.read(spool::SPOOL_HEADER.len() as u64)?;
        let memory = control.memory.spool.reserve(std::mem::size_of::<Self>())?;
        let cursor = spool::SpoolSource {
            path: &run.path,
            document_count: 0,
            max_record_bytes: writer.options.max_record_bytes.get(),
            max_metadata_fields: writer.options.max_metadata_fields.get(),
            memory: control.memory.clone(),
        }
        .cursor(&control.task)?;
        if cursor.length() != run.bytes {
            return Err(invalid("run length changed"));
        }
        Ok(Self {
            cursor,
            run,
            position: spool::SPOOL_HEADER.len() as u64,
            records: 0,
            digest: Default::default(),
            previous: None,
            memory: control.memory.clone(),
            task: control.task.clone(),
            _memory: memory,
        })
    }
    fn raw(&self, offset: u64, bytes: &mut [u8], control: &mut Control) -> Result<()> {
        control.read(bytes.len() as u64)?;
        self.cursor.read_exact_at(offset, bytes)
    }
    pub(super) fn next(&mut self, control: &mut Control) -> Result<Option<Entry>> {
        if self.records == self.run.records {
            if self.position != self.run.bytes || self.digest != self.run.digest {
                return Err(invalid("run is incomplete, reordered or corrupted"));
            }
            return Ok(None);
        }
        control.visit()?;
        let start = self.position;
        let mut tag = [0];
        self.raw(start, &mut tag, control)?;
        let (entry, end) = match tag[0] {
            0 => {
                let mut length = [0; 8];
                self.raw(start + 1, &mut length, control)?;
                let length = u64::from_le_bytes(length);
                let size =
                    usize::try_from(length).map_err(|_| invalid("ID length exceeds usize"))?;
                if size == 0 || size > control.max_id_bytes {
                    return Err(invalid("ID exceeds admission"));
                }
                let end = start
                    .checked_add(17)
                    .and_then(|n| n.checked_add(length))
                    .filter(|n| *n <= self.run.bytes)
                    .ok_or_else(|| invalid("delete is truncated"))?;
                let _scratch = self.memory.spool.reserve(size)?;
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(size)
                    .map_err(|_| invalid("cannot allocate ID input"))?;
                if bytes.capacity() != size {
                    return Err(invalid("ID input exceeds admission"));
                }
                bytes.resize(size, 0);
                self.raw(start + 9, &mut bytes, control)?;
                let mut checksum = [0; 8];
                self.raw(start + 9 + length, &mut checksum, control)?;
                let mut digest = Crc32cHasher::new();
                digest.update(&bytes);
                if digest.finish() != u64::from_le_bytes(checksum) {
                    return Err(invalid("delete checksum mismatch"));
                }
                let id = std::str::from_utf8(&bytes).map_err(|_| invalid("ID is not UTF-8"))?;
                let id = Term::copy(id, Some(&self.memory))?;
                let mut record = DocumentsDigest::default();
                record.add_record(digest.finish(), length);
                evidence(&mut self.digest, record.finish(), end - start, 0);
                (Entry::Delete(id), end)
            }
            1 => {
                let mut flag = [0];
                self.raw(start + 1, &mut flag, control)?;
                if flag[0] > 1 {
                    return Err(invalid("invalid analyzer marker"));
                }
                let mut frame = [0; 16];
                self.raw(start + 2, &mut frame, control)?;
                let length = u64::from_le_bytes(frame[..8].try_into().unwrap());
                let end = start
                    .checked_add(18)
                    .and_then(|n| n.checked_add(length))
                    .filter(|n| *n <= self.run.bytes)
                    .ok_or_else(|| invalid("upsert is truncated"))?;
                control.read(
                    length
                        .checked_add(16)
                        .ok_or_else(|| invalid("read extent overflow"))?,
                )?;
                let record = self.cursor.record_at(start + 2, end)?;
                let mut value = DocumentsDigest::default();
                record.add_digest(&mut value);
                evidence(
                    &mut self.digest,
                    value.finish(),
                    end - start,
                    2 + u64::from(flag[0]),
                );
                (Entry::Upsert(Box::new(record), flag[0] == 1), end)
            }
            _ => return Err(invalid("invalid operation tag")),
        };
        validate_id(entry.id(), control.max_id_bytes)?;
        if self
            .previous
            .as_ref()
            .is_some_and(|previous| &**previous > entry.id())
        {
            return Err(invalid("run IDs are not ordered"));
        }
        self.previous = Some(Term::copy(entry.id(), Some(&self.memory))?);
        self.position = end;
        self.records += 1;
        Ok(Some(entry))
    }
    pub(super) fn remove(self, control: &Control) -> Result<()> {
        let path = self.run.path;
        drop(self.cursor);
        io::GenerationIo::new(&control.memory, &self.task)
            .native(&[&path], || fs::remove_file(&*path))??;
        Ok(())
    }
}

pub(super) fn merge(
    left: Run,
    right: Run,
    control: &mut Control,
    writer: &mut SearchOutOfCoreGenerationWriter,
) -> Result<Run> {
    let mut left = Reader::open(left, control, writer)?;
    let mut right = Reader::open(right, control, writer)?;
    let mut output = control.output(writer)?;
    let (mut a, mut b) = (left.next(control)?, right.next(control)?);
    while a.is_some() || b.is_some() {
        checkpoint(&control.task)?;
        let from_left = match (&a, &b) {
            (Some(a), Some(b)) => a.id() <= b.id(),
            (Some(_), None) => true,
            _ => false,
        };
        if from_left {
            output.copied(a.as_ref().unwrap(), control, writer)?;
            a = left.next(control)?;
        } else {
            output.copied(b.as_ref().unwrap(), control, writer)?;
            b = right.next(control)?;
        }
    }
    let run = output.finish()?;
    left.remove(control)?;
    right.remove(control)?;
    Ok(run)
}
pub(super) fn reduce(left: Entry, right: Entry, control: &mut Control) -> Result<Entry> {
    match (left, right) {
        (Entry::Delete(_), upsert @ Entry::Upsert(..))
        | (upsert @ Entry::Upsert(..), Entry::Delete(_)) => Ok(upsert),
        (Entry::Delete(left), Entry::Delete(_)) => Ok(Entry::Delete(left)),
        (Entry::Upsert(left, a), Entry::Upsert(right, b)) => {
            control.read(
                left.encoded_bytes
                    .checked_add(right.encoded_bytes)
                    .ok_or_else(|| invalid("comparison extent overflow"))?,
            )?;
            if !left.encoded_eq(&right, &control.memory, &control.task)? {
                return Err(invalid("conflicting upserts for one document ID"));
            }
            Ok(Entry::Upsert(left, a || b))
        }
    }
}

#[cfg(test)]
impl Run {
    pub(super) fn path_for_test(&self) -> &std::path::Path {
        &self.path
    }
}
