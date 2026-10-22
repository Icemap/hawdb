// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Schema-bound vector index definitions shared by SQL and Cypher.
//!
//! A descriptor is a definition, not serving authority. Publication identity,
//! source epoch and ordinal ownership belong to its registered Consumer pin.

use crate::{HawDBError, PropertyId, Result, TableId, VectorDimension, VectorDistance};

mod codec;

/// Upper bound used to admit descriptor encoding/decoding before allocation.
pub const MAX_VECTOR_INDEX_DESCRIPTOR_BYTES: usize = 4096;

/// Resolved field identity. Relational schemas currently identify columns by
/// normalized table name and ordinal; the descriptor's schema digest fences
/// reinterpretation of that ordinal after a schema change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorIndexTarget {
    Relational {
        table: String,
        column: String,
        column_ordinal: u32,
    },
    NodeProperty {
        table: String,
        property: String,
        table_id: TableId,
        property_id: PropertyId,
    },
}

/// Persistent algorithm settings, independent of per-operation admission.
/// Runtime memory limits and cancellation are supplied at build/query time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorIndexBackend {
    RaBitQ {
        bit_width: u8,
        segment_rows: u32,
        transform_seed: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorIndexDescriptor {
    name: String,
    target: VectorIndexTarget,
    dimension: VectorDimension,
    distance: VectorDistance,
    backend: VectorIndexBackend,
    schema_digest: [u8; 32],
}

impl VectorIndexDescriptor {
    /// Constructs a validated definition. The facade must resolve the target,
    /// dimension and digest against its pinned canonical schema before use.
    pub fn new(
        name: String,
        target: VectorIndexTarget,
        dimension: VectorDimension,
        distance: VectorDistance,
        backend: VectorIndexBackend,
        schema_digest: [u8; 32],
    ) -> Result<Self> {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(invalid(
                "name must be 1..128 ASCII letters, digits, '.', '_' or '-'",
            ));
        }
        let (table, field) = match &target {
            VectorIndexTarget::Relational { table, column, .. } => (table, column),
            VectorIndexTarget::NodeProperty {
                table, property, ..
            } => (table, property),
        };
        for name in [table, field] {
            if name.is_empty() || name.len() > 1024 || name.contains('\0') {
                return Err(invalid(
                    "target names must contain 1..1024 non-NUL UTF-8 bytes",
                ));
            }
        }
        let VectorIndexBackend::RaBitQ {
            bit_width,
            segment_rows,
            ..
        } = backend;
        if !matches!(bit_width, 1 | 4) || segment_rows == 0 {
            return Err(invalid(
                "RaBitQ requires 1 or 4 bits and a positive segment size",
            ));
        }
        if distance != VectorDistance::Cosine {
            return Err(invalid(
                "RaBitQ indexes currently support cosine distance only",
            ));
        }
        Ok(Self {
            name,
            target,
            dimension,
            distance,
            backend,
            schema_digest,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn target(&self) -> &VectorIndexTarget {
        &self.target
    }

    pub const fn dimension(&self) -> VectorDimension {
        self.dimension
    }

    pub const fn distance(&self) -> VectorDistance {
        self.distance
    }

    pub const fn backend(&self) -> VectorIndexBackend {
        self.backend
    }

    pub const fn schema_digest(&self) -> &[u8; 32] {
        &self.schema_digest
    }

    pub fn retained_bytes(&self) -> usize {
        let (table, field) = match &self.target {
            VectorIndexTarget::Relational { table, column, .. } => (table, column),
            VectorIndexTarget::NodeProperty {
                table, property, ..
            } => (table, property),
        };
        std::mem::size_of::<Self>() + self.name.capacity() + table.capacity() + field.capacity()
    }
}

fn invalid(reason: &str) -> HawDBError {
    HawDBError::Semantic(format!("invalid vector index: {reason}"))
}
