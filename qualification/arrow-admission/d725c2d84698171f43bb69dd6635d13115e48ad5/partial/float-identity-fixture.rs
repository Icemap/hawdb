#[test]
fn float_and_identity_arrow_payload_snapshot_for_cross_revision_compatibility() {
    let mut db = Database::new();
    let governor = governor_with_handles(1024);
    db.set_runtime_governor(governor.clone());
    db.query("CREATE NODE TABLE Item").unwrap();
    db.query("CREATE PROPERTY ON NODE TABLE Item(score) TYPE FLOAT")
        .unwrap();
    let bits = [
        0x8000000000000000,
        0,
        0x7ff800000000002a,
        0x7ff0000000000000,
    ];
    for value in std::iter::once(Value::Null).chain(
        bits.into_iter()
            .map(|word| Value::Float(f64::from_bits(word))),
    ) {
        db.query_with_params(
            "CREATE (:Item {score: $value})",
            &BTreeMap::from([("value".into(), value)]),
        )
        .unwrap();
    }
    let mut cursor = db
        .query_with_params_retained(
            QUERY,
            &BTreeMap::from([("min".into(), Value::Float(f64::NEG_INFINITY))]),
            RetainedQueryOptions {
                batch_rows: nz(5),
                ..options()
            },
        )
        .unwrap();
    let batch = cursor.next_batch().unwrap().unwrap();
    assert_eq!(batch.selected_rows(), &[1, 2, 3, 4]);
    let export = batch.export_arrow(0).unwrap();
    assert!(cursor.next_batch().unwrap().is_none());
    drop(cursor);
    drop(db);
    let mut columns = Vec::new();
    for index in 0..3 {
        // The descriptor owns the buffers after database/cursor closure. Bounds
        // come from the retained batch and the verified contiguous selection.
        let (name, format, flags, offset, length, null_count, metadata, values, validity) = unsafe {
            let field = &**export.schema().children.add(index);
            let array = &**export.array().children.add(index);
            assert_eq!(array.n_buffers, 2);
            assert_eq!(array.offset, 1);
            assert_eq!(array.length, 4);
            let expected_pointer = match batch.column(index).unwrap() {
                RetainedColumnValues::Float64(values) => values.as_ptr().cast::<u8>(),
                RetainedColumnValues::UInt64(values) => values.as_ptr().cast::<u8>(),
                _ => panic!("Float64 or UInt64 fixture column"),
            };
            assert_eq!((*array.buffers.add(1)).cast::<u8>(), expected_pointer);
            let values = std::slice::from_raw_parts(expected_pointer.add(8), 4 * 8).to_vec();
            let validity = if (*array.buffers).is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts((*array.buffers).cast::<u8>(), 1).to_vec()
            };
            let metadata = if field.metadata.is_null() {
                Vec::new()
            } else {
                // Standard metadata: three i32 fields, ten key bytes and
                // thirteen value bytes; pointer addresses are never compared
                // across revisions.
                std::slice::from_raw_parts(field.metadata.cast::<u8>(), 35).to_vec()
            };
            (
                CStr::from_ptr(field.name).to_str().unwrap().to_owned(),
                CStr::from_ptr(field.format).to_str().unwrap().to_owned(),
                field.flags,
                array.offset,
                array.length,
                array.null_count,
                metadata,
                values,
                validity,
            )
        };
        let provenance = export.column_provenance(index).unwrap();
        assert_eq!(provenance.byte_offset, 8);
        assert_eq!(provenance.byte_length, 32);
        if index == 1 {
            assert_eq!(format, "L");
            assert_eq!(null_count, 0);
            assert!(validity.is_empty());
            assert_eq!(&metadata[8..18], b"hawdb:role");
            assert_eq!(&metadata[22..], b"node_identity");
        } else {
            assert_eq!(format, "g");
            assert_eq!(null_count, -1);
            assert_eq!(validity, [30]);
            assert!(metadata.is_empty());
            assert_eq!(
                values,
                bits.into_iter()
                    .flat_map(u64::to_le_bytes)
                    .collect::<Vec<_>>()
            );
        }
        columns.push(serde_json::json!({
            "name": name, "format": format, "flags": flags,
            "offset": offset, "length": length, "null_count": null_count,
            "metadata": metadata, "values": values, "validity": validity,
            "provenance_byte_offset": provenance.byte_offset,
            "provenance_byte_length": provenance.byte_length,
        }));
    }
    println!(
        "ARROW_FLOAT_IDENTITY_SNAPSHOT={}",
        serde_json::json!({
            "root_length": export.array().length,
            "root_offset": export.array().offset,
            "root_children": export.array().n_children,
            "columns": columns,
        })
    );
    drop(export);
    drop(batch);
    let snapshot = governor.retained_result_snapshot();
    assert_eq!(snapshot.retained_bytes, 0);
    assert_eq!(snapshot.buffer_owners, 0);
    assert_eq!(snapshot.view_handles, 0);
}
