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

//! Fresh query admission for immutable mutation-term ranges.
use super::{terms::TermFile, HawDBError, Result};
use crate::lexical_projection::QueryContext;
use hawdb_executor::QueryMemoryLease;
use hawdb_storage::file_io::ReadAdmissionControl;
use std::io::{self, Read};
use std::num::NonZeroUsize;

pub(in crate::out_of_core) struct Range<'a> {
    pub(super) file: &'a TermFile,
    pub(super) offset: u64,
    pub(super) remaining: u64,
    pub(super) context: Option<QueryContext<'a>>,
}
impl Read for Range<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.remaining.min(output.len() as u64).min(8192) as usize;
        if count == 0 {
            return Ok(0);
        }
        let count = match self.context {
            None => {
                if self.file.requires_context() {
                    return Err(io::Error::other(HawDBError::Execution(
                        "admitted mutation terms require a fresh query context".into(),
                    )));
                }
                let mut reader = super::super::hydration::RangeReader {
                    file: &self.file.file,
                    offset: self.offset,
                    remaining: self.remaining,
                };
                reader.read(&mut output[..count])?
            }
            Some(context) => read_at(self.file, context, &mut output[..count], self.offset)
                .map_err(io::Error::other)?,
        };
        self.offset = self
            .offset
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("mutation range offset overflow"))?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

fn read_at(
    file: &TermFile,
    context: QueryContext<'_>,
    output: &mut [u8],
    offset: u64,
) -> Result<usize> {
    context.checkpoint()?;
    if file.requires_context() && context.execution.is_none() {
        return Err(HawDBError::Execution(
            "admitted mutation terms require borrowed query execution".into(),
        ));
    }
    let _io = context
        .task
        .acquire_io_wave(NonZeroUsize::MIN)
        .map_err(|error| HawDBError::Execution(error.to_string()))?;
    if file.requires_context() && _io.is_none() {
        return Err(HawDBError::Execution(
            "admitted mutation terms require query I/O admission".into(),
        ));
    }
    let mut control = Control {
        file,
        context,
        validation: None,
        error: None,
    };
    let result = file.file.with_admitted_read(&mut control, |reader| {
        if let Some(expected) = file.expected_length
            && reader.metadata()?.len() != expected
        {
            return Err(io::Error::other(HawDBError::Storage(
                "mutation term artifact length changed".into(),
            )));
        }
        reader.read_at(output, offset)
    });
    context.checkpoint()?;
    if let Some(error) = control.error.take() {
        return Err(error);
    }
    result.map(|(count, _)| count).map_err(io_error)
}

struct Control<'a> {
    file: &'a TermFile,
    context: QueryContext<'a>,
    validation: Option<QueryMemoryLease>,
    error: Option<HawDBError>,
}
impl ReadAdmissionControl for Control<'_> {
    fn admit_validation(&mut self, bytes: u64) -> io::Result<()> {
        let result = (|| {
            if self
                .file
                .expected_length
                .is_some_and(|expected| expected != bytes)
            {
                return Err(HawDBError::Storage(
                    "mutation term object length changed".into(),
                ));
            }
            self.validation = Some(self.context.memory.reserve(64 << 10)?);
            Ok(())
        })();
        result.map_err(|error| {
            self.error = Some(error);
            io::Error::other("mutation read admission refused")
        })
    }
    fn checkpoint(&mut self) -> io::Result<()> {
        self.context.checkpoint().map_err(io::Error::other)
    }
}

pub(super) fn io_error(error: io::Error) -> HawDBError {
    if error
        .get_ref()
        .is_some_and(|inner| inner.is::<HawDBError>())
    {
        return *error
            .into_inner()
            .expect("checked I/O payload")
            .downcast::<HawDBError>()
            .expect("checked error type");
    }
    HawDBError::from(error)
}

pub(super) fn json_error(error: serde_json::Error, label: &str) -> HawDBError {
    if error.is_io() {
        io_error(error.into())
    } else {
        HawDBError::Storage(format!("{label}: {error}"))
    }
}
