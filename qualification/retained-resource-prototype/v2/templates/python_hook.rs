/// Private qualification hook; absent from ordinary/default/lite builds.
#[cfg(feature = "retained-phase-profiling")]
#[pyfunction]
fn _retained_phase_snapshot(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let snapshot = hawdb::retained_phase_profile::snapshot();
    let result = PyDict::new(py);
    result.set_item("protocol", "hawdb-retained-phase-v2")?;
    result.set_item(
        "allocation_instrumented",
        cfg!(feature = "boundary-profiling"),
    )?;
    result.set_item("overflowed", snapshot.overflowed)?;
    for (key, value) in [
        ("prepare_calls", snapshot.prepare_calls),
        ("prepare_ns", snapshot.prepare_ns),
        ("pull_calls", snapshot.pull_calls),
        ("pull_ns", snapshot.pull_ns),
        ("owned_calls", snapshot.owned_calls),
        ("owned_ns", snapshot.owned_ns),
    ] {
        result.set_item(key, value)?;
    }
    Ok(result)
}
