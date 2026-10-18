# Experimental retained Arrow handoff

Issue Number: ref #987

The embedded Rust library exports compatible retained numeric batches through
the Arrow C Data and C Stream interfaces. Python exposes the corresponding
`__arrow_c_array__`, `__arrow_c_schema__` and `__arrow_c_stream__` protocols.
This builds on the [numeric core](RETAINED_NUMERIC_CORE.md); indexed complete-node
views and the complete #986 performance/resource gate remain separate work.

Arrays retain native payload owners rather than serializing rows or IPC.
Descriptors and moved children follow Arrow's single-release contract. A host
that can unload its callback code supplies a `RetainedArrowCodeOwner`; Python
protocol objects keep the extension module alive. Previously delivered arrays
remain readable after cursor, stream or database closure.

Schema and descriptor ownership consumes the existing query/runtime allowances.
Stream adoption checks the irreducible metadata for its descriptor, schema and
minimum demanded batch against the existing allowances. An impossible working
unit refuses before taking the caller's cursor; releasable arrays and schemas
can still cause retryable pressure. Empty sources can complete without a batch.
Direct C Data export also checks the borrowed native batch plus its complete
schema, array and retained-view metadata before constructing descriptors. A
schema-only export checks the input it must keep alive during that call.
Stream callbacks recheck their working unit after a host resource refresh;
permanent schema or array admission failures terminate without source advance.
Export may refuse before adoption when byte, slot or handle capacity is missing.
It does not increase those allowances, prefetch data or silently gather rows.
Successful EOF is required before treating all earlier batches as a complete
result. Source and buffer accounting does not bound interpreter or process RSS.

Data exports require a little-endian host and an empty or contiguous selection.
Selection contiguity is checked when each batch is demanded: a stream can deliver
an earlier array and then fail with errno 22 on a later sparse selection. The
earlier array remains readable but is a partial result. Errno 12 covers both
retryable pressure and terminal capacity/budget failures; inspect the diagnostic
and typed status rather than inferring retryability from errno alone.

Rust hosts should use `RetainedQueryError::is_retryable()`. In particular,
`RetainedQueryError::RetainedAdmission(RuntimeRetainedResultError::Capacity {
retryable: true, .. })` represents releasable pressure. Release other live
owners before retrying; repeated calls cannot cure an intrinsically oversized
working unit. `RetainedQueryError::WorkingUnitTooLarge` is nonretryable.
`get_schema` and `get_next` latch nonretryable errors and abort delivery on an
open cursor; already completed EOF remains completed. Earlier arrays keep their
payload owners but do not establish successful completion after an error.

The existing native retained C ABI and Go numeric API remain independent of
these Rust descriptors and Python protocols. No Arrow library is required by
HawDB or the default Python extension. Optional public-consumer tests provision
PyArrow explicitly; they do not make it a production dependency.

This additive experimental API does not change persistence formats, durability
defaults, ordinary query routing or Mem release/activation policy. Stable Mem
artifacts must continue to exclude HawDB until separately authorized.
