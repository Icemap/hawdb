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

//! Internal Zstd streams whose native contexts use the host's Rust allocator.
//!
//! Keep admission at the caller: selecting an allocation backend does not
//! reserve query memory or charge a second time for an admitted workspace.

mod context;
mod output;
use crate::background::{CheckpointWorkContext, CheckpointWorkError};
use context::CompressionContext;
pub use context::DecompressionContext;
use hawdb_core::RuntimeMemoryPermit;
pub(crate) use output::AdmittedCompressionOutput;
use std::io::{self, BufRead, BufReader, Read, Write};
use zstd::stream::raw::{InBuffer, Operation, OutBuffer, WriteBuf};
use zstd::stream::zio;

pub struct Encoder<W: Write> {
    writer: zio::Writer<W, CompressionContext>,
    _buffer_memory: Option<Box<dyn RuntimeMemoryPermit>>,
}

impl<W: Write> Encoder<W> {
    pub fn new(writer: W, level: i32) -> io::Result<Self> {
        Ok(Self {
            writer: zio::Writer::new(writer, CompressionContext::new(level)?),
            _buffer_memory: None,
        })
    }

    pub(crate) fn new_with_work_context(
        writer: W,
        level: i32,
        work: &CheckpointWorkContext,
    ) -> io::Result<Self> {
        Self::with_staging_capacity(writer, level, 32 * 1024, work)
    }

    pub(crate) fn with_staging_capacity(
        writer: W,
        level: i32,
        capacity: usize,
        work: &CheckpointWorkContext,
    ) -> io::Result<Self> {
        if capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty Zstd staging buffer",
            ));
        }
        let memory = work.reserve_memory(capacity).map_err(io::Error::other)?;
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(capacity).map_err(|error| {
            io::Error::other(work.record_failure(CheckpointWorkError::Allocation {
                bytes: capacity as u64,
                reason: error.to_string(),
            }))
        })?;
        if buffer.capacity() != capacity {
            return Err(io::Error::other(work.record_failure(
                CheckpointWorkError::Allocation {
                    bytes: capacity as u64,
                    reason: "Zstd output buffer exceeds admitted capacity".into(),
                },
            )));
        }
        Ok(Self {
            writer: zio::Writer::with_output_buffer(
                buffer,
                writer,
                CompressionContext::new_with_work_context(level, work)?,
            ),
            _buffer_memory: memory,
        })
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.writer.finish()?;
        let (writer, context) = self.writer.into_inner();
        // Release native workspace before the caller releases its admission.
        context.close()?;
        Ok(writer)
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writer.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub struct Decoder<R> {
    reader: zio::Reader<R, DecompressionContext>,
}

impl<R: Read> Decoder<BufReader<R>> {
    pub fn new(reader: R) -> io::Result<Self> {
        Self::with_buffer(BufReader::with_capacity(
            zstd::zstd_safe::DCtx::in_size(),
            reader,
        ))
    }
}

impl<R: BufRead> Decoder<R> {
    pub fn with_buffer(reader: R) -> io::Result<Self> {
        Ok(Self {
            reader: zio::Reader::new(reader, DecompressionContext::new()?),
        })
    }
}

impl<R: BufRead> Read for Decoder<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.reader.read(bytes)
    }
}

impl Operation for DecompressionContext {
    fn run<C: WriteBuf + ?Sized>(
        &mut self,
        input: &mut InBuffer<'_>,
        output: &mut OutBuffer<'_, C>,
    ) -> io::Result<usize> {
        self.decompress_stream(output, input)
    }

    fn flush<C: WriteBuf + ?Sized>(&mut self, output: &mut OutBuffer<'_, C>) -> io::Result<usize> {
        self.run(&mut InBuffer::around(&[]), output)?;
        Ok(usize::from(output.pos() == output.capacity()))
    }

    fn reinit(&mut self) -> io::Result<()> {
        self.reset()
    }

    fn finish<C: WriteBuf + ?Sized>(
        &mut self,
        _output: &mut OutBuffer<'_, C>,
        finished_frame: bool,
    ) -> io::Result<usize> {
        if finished_frame {
            Ok(0)
        } else {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete frame",
            ))
        }
    }
}

pub fn encode_all(mut source: impl Read, level: i32) -> io::Result<Vec<u8>> {
    let mut encoder = Encoder::new(Vec::new(), level)?;
    io::copy(&mut source, &mut encoder)?;
    encoder.finish()
}

#[cfg(test)]
mod tests;
