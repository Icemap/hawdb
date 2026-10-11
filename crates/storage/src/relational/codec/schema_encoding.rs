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

//! One borrowed schema/value grammar for ordinary and checkpoint writers.

use super::*;

pub(crate) trait Sink {
    fn raw(&mut self, bytes: &[u8]) -> Result<(), RelationalError>;

    fn value(&mut self, item: &RelationalValue) -> Result<(), RelationalError>
    where
        Self: Sized,
    {
        value(self, item)
    }
}

pub(crate) struct Schema<'a> {
    pub(crate) name: &'a str,
    pub(crate) columns: &'a [RelationalColumnSchema],
    pub(crate) primary_key: &'a [String],
    pub(crate) primary_key_suffix: &'a [String],
    pub(crate) unique_constraints: &'a [Vec<String>],
    pub(crate) foreign_keys: &'a [RelationalForeignKeySchema],
    pub(crate) indexes: &'a [RelationalIndexSchema],
}

impl<'a> From<&'a RelationalTableSchema> for Schema<'a> {
    fn from(schema: &'a RelationalTableSchema) -> Self {
        Self {
            name: &schema.name,
            columns: &schema.columns,
            primary_key: &schema.primary_key,
            primary_key_suffix: &[],
            unique_constraints: &schema.unique_constraints,
            foreign_keys: &schema.foreign_keys,
            indexes: &schema.indexes,
        }
    }
}

pub(crate) fn count(
    sink: &mut impl Sink,
    len: usize,
    context: &str,
) -> Result<(), RelationalError> {
    let count = u32::try_from(len).map_err(|_| {
        RelationalError::Admission(format!("{context} count exceeds durable codec limit"))
    })?;
    sink.raw(&count.to_le_bytes())
}

fn bytes(sink: &mut impl Sink, data: &[u8]) -> Result<(), RelationalError> {
    let len = u64::try_from(data.len())
        .map_err(|_| RelationalError::Admission("durable byte string is too large".into()))?;
    sink.raw(&len.to_le_bytes())?;
    sink.raw(data)
}

fn strings(sink: &mut impl Sink, values: &[String]) -> Result<(), RelationalError> {
    count(sink, values.len(), "string list")?;
    for item in values {
        bytes(sink, item.as_bytes())?;
    }
    Ok(())
}

pub(crate) fn row(sink: &mut impl Sink, row: &RelationalRow) -> Result<(), RelationalError> {
    count(sink, row.values().len(), "row values")?;
    for item in row.values() {
        sink.value(item)?;
    }
    Ok(())
}

pub(crate) fn scalar_tag(kind: RelationalScalarType) -> u8 {
    match kind {
        RelationalScalarType::Boolean => 1,
        RelationalScalarType::BigInt => 2,
        RelationalScalarType::DoublePrecision => 3,
        RelationalScalarType::Text => 4,
        RelationalScalarType::Bytea => 5,
        RelationalScalarType::Uuid => 6,
    }
}

pub(crate) fn value(sink: &mut impl Sink, item: &RelationalValue) -> Result<(), RelationalError> {
    match item {
        RelationalValue::Null => sink.raw(&[0]),
        RelationalValue::Boolean(value) => sink.raw(&[1, u8::from(*value)]),
        RelationalValue::BigInt(value) => {
            sink.raw(&[2])?;
            sink.raw(&value.to_le_bytes())
        }
        RelationalValue::DoublePrecision(value) => {
            sink.raw(&[3])?;
            sink.raw(&value.to_bits().to_le_bytes())
        }
        RelationalValue::Text(value) => {
            sink.raw(&[4])?;
            bytes(sink, value.as_bytes())
        }
        RelationalValue::Bytea(value) => {
            sink.raw(&[5])?;
            bytes(sink, value)
        }
        RelationalValue::Uuid(value) => {
            sink.raw(&[7])?;
            sink.raw(value.as_bytes())
        }
        RelationalValue::Overflow(reference) => {
            sink.raw(&[6])?;
            sink.raw(reference.digest.as_bytes())?;
            sink.raw(&[scalar_tag(reference.scalar_type)])?;
            sink.raw(&reference.compressed_bytes.to_le_bytes())?;
            sink.raw(&reference.uncompressed_bytes.to_le_bytes())
        }
    }
}

fn action(value: RelationalReferentialAction) -> u8 {
    match value {
        RelationalReferentialAction::NoAction => 0,
        RelationalReferentialAction::Restrict => 1,
        RelationalReferentialAction::Cascade => 2,
    }
}

pub(crate) fn schema(sink: &mut impl Sink, schema: Schema<'_>) -> Result<(), RelationalError> {
    bytes(sink, schema.name.as_bytes())?;
    count(sink, schema.columns.len(), "table columns")?;
    for column in schema.columns {
        bytes(sink, column.name.as_bytes())?;
        sink.raw(&[scalar_tag(column.scalar_type), u8::from(column.nullable)])?;
        match &column.default {
            None => sink.raw(&[0])?,
            Some(RelationalColumnDefault::UuidV7) => sink.raw(&[2])?,
            Some(RelationalColumnDefault::Literal(value)) => {
                sink.raw(&[1])?;
                sink.value(value)?;
            }
        }
    }
    let keys = schema
        .primary_key
        .len()
        .checked_add(schema.primary_key_suffix.len())
        .ok_or_else(|| {
            RelationalError::Admission("string list count exceeds durable codec limit".into())
        })?;
    count(sink, keys, "string list")?;
    for key in schema.primary_key.iter().chain(schema.primary_key_suffix) {
        bytes(sink, key.as_bytes())?;
    }
    count(sink, schema.unique_constraints.len(), "unique constraints")?;
    for columns in schema.unique_constraints {
        strings(sink, columns)?;
    }
    count(sink, schema.foreign_keys.len(), "foreign keys")?;
    for foreign in schema.foreign_keys {
        strings(sink, &foreign.columns)?;
        bytes(sink, foreign.referenced_table.as_bytes())?;
        strings(sink, &foreign.referenced_columns)?;
        sink.raw(&[action(foreign.on_delete), action(foreign.on_update)])?;
    }
    count(sink, schema.indexes.len(), "indexes")?;
    for index in schema.indexes {
        bytes(sink, index.name.as_bytes())?;
        strings(sink, &index.columns)?;
        sink.raw(&[u8::from(index.unique)])?;
    }
    Ok(())
}

impl Sink for Encoder {
    fn raw(&mut self, bytes: &[u8]) -> Result<(), RelationalError> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn value(&mut self, item: &RelationalValue) -> Result<(), RelationalError> {
        Encoder::value(self, item)
    }
}
