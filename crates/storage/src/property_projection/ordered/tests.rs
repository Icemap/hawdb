// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::cell::Cell;
use std::rc::Rc;

struct Fixture {
    reader: PersistentPropertyProjectionReader,
    properties: Vec<String>,
    equality: Vec<(String, Value)>,
    _root: FixtureRoot,
}

struct FixtureRoot(PathBuf);
impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn new() -> Self {
        Self::with_block_bytes(256, 1024 * 1024)
    }

    fn with_block_bytes(target: u64, cache_bytes: u64) -> Self {
        let root = std::env::temp_dir().join(format!(
            "hawdb-ordered-composite-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        fs::create_dir(&root).unwrap();
        let properties = vec!["kind".into(), "rank".into(), "id".into()];
        let definitions = vec![
            PersistentPropertyProjectionDefinition {
                label_id: LabelId(1),
                property: "rank".into(),
                kind: PersistentPropertyProjectionKind::Range,
                complete: false,
            },
            PersistentPropertyProjectionDefinition {
                label_id: LabelId(1),
                property: persistent_composite_property_identity(&properties).unwrap(),
                kind: PersistentPropertyProjectionKind::CompositeEquality,
                complete: false,
            },
        ];
        let paths = GraphDescriptorTreePaths::new(
            root.join("projection-descriptors.pages"),
            root.join("projection-root.root"),
        );
        let path = root.join("projection.hawdb");
        let output = PersistentPropertyProjectionWriter::new(PersistentPropertyProjectionConfig {
            target_block_bytes: NonZeroU64::new(target).unwrap(),
            ..Default::default()
        })
        .write_fallible(
            &path,
            ManifestGeneration(9),
            31,
            definitions,
            (0u64..512).map(|id| {
                Ok(PersistentPropertyProjectionRecord::Node(NodeRecord {
                    id: NodeId(id),
                    labels: BTreeSet::from([LabelId(1)]),
                    properties: BTreeMap::from([
                        (
                            "kind".into(),
                            Value::String(if id < 256 { "archive" } else { "note" }.into()),
                        ),
                        ("rank".into(), Value::Int((id / 4) as i64)),
                        ("id".into(), Value::String(format!("item-{id:04}"))),
                    ]),
                }))
            }),
            PersistentPropertyProjectionDescriptorTree::new(
                paths.clone(),
                GraphDescriptorTreeBuildConfig::default(),
            ),
        )
        .unwrap();
        if target == 256 {
            assert!(output.manifest.block_count > 100);
        }
        let reader = PersistentPropertyProjectionReader::open(
            &path,
            output.manifest,
            PersistentPropertyProjectionDescriptorTree::new(
                paths,
                GraphDescriptorTreeBuildConfig::default(),
            ),
            Arc::new(SegmentCache::new(cache_bytes)),
            StoreId(27),
            NonZeroU64::new(1024 * 1024).unwrap(),
        )
        .unwrap();
        Self {
            _root: FixtureRoot(root),
            reader,
            properties,
            equality: vec![("kind".into(), Value::String("note".into()))],
        }
    }

    fn order<'a>(&'a self, descending: bool, after: Option<&'a [Value]>) -> CompositeOrder<'a> {
        CompositeOrder {
            label: LabelId(1),
            properties: &self.properties,
            equality: &self.equality,
            after,
            descending,
        }
    }
}

struct Lease {
    current: Rc<Cell<usize>>,
    bytes: usize,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.current.set(self.current.get() - self.bytes);
    }
}

#[test]
fn composite_ordered_pages_match_both_directions_and_seek_deep_blocks() {
    let fixture = Fixture::new();
    for descending in [false, true] {
        let mut expected = (256u64..512).collect::<Vec<_>>();
        if descending {
            expected.reverse();
        }
        let current = Rc::new(Cell::new(0usize));
        let peak = Cell::new(0usize);
        let mut actual = Vec::new();
        let mut after = None;
        loop {
            let mut page = Vec::new();
            let mut last = None;
            let (report, control) = fixture
                .reader
                .scan_composite_ordered_entries(
                    fixture.order(descending, after.as_deref()),
                    |bytes| {
                        current.set(current.get().checked_add(bytes).unwrap());
                        peak.set(peak.get().max(current.get()));
                        Ok(Lease {
                            current: Rc::clone(&current),
                            bytes,
                        })
                    },
                    |key, id| {
                        let Value::List(values) = key else {
                            panic!("composite key required");
                        };
                        last = Some(values[1..].to_vec());
                        page.push(id.0);
                        Ok(if page.len() == 5 {
                            CanonicalScanControl::Stop
                        } else {
                            CanonicalScanControl::Continue
                        })
                    },
                )
                .unwrap();
            assert_eq!(current.get(), 0);
            assert_eq!(report.candidates_returned as usize, page.len());
            assert!(
                report.descriptors_examined < 32,
                "deep page must not enumerate its skipped prefix: {report:?}"
            );
            assert!(report.blocks_read <= 6, "{report:?}");
            actual.extend(page);
            assert!(actual.len() <= expected.len());
            if control == CanonicalScanControl::Continue {
                break;
            }
            after = last;
        }
        assert_eq!(actual, expected);
        assert!(
            peak.get() < 32 * 1024,
            "bounded block scratch: {}",
            peak.get()
        );
    }
}

#[test]
fn ordered_composite_refusal_and_early_stop_release_scratch_without_poisoning() {
    let fixture = Fixture::new();
    let current = Rc::new(Cell::new(0usize));
    let calls = Cell::new(0);
    let error = fixture
        .reader
        .scan_composite_ordered_entries(
            fixture.order(true, None),
            |bytes| {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    return Err(PersistentPropertyProjectionError::Source(
                        "caller scratch refusal".into(),
                    ));
                }
                current.set(current.get() + bytes);
                Ok(Lease {
                    current: Rc::clone(&current),
                    bytes,
                })
            },
            |_, _| panic!("refusal must precede candidate delivery"),
        )
        .unwrap_err();
    assert!(error.to_string().contains("caller scratch refusal"));
    assert_eq!(current.get(), 0);
    assert!(!fixture.reader.is_poisoned());
    let mut ids = Vec::new();
    let (_, control) = fixture
        .reader
        .scan_composite_ordered_entries(
            fixture.order(true, None),
            |bytes| {
                current.set(current.get() + bytes);
                Ok(Lease {
                    current: Rc::clone(&current),
                    bytes,
                })
            },
            |_, id| {
                ids.push(id.0);
                Ok(CanonicalScanControl::Stop)
            },
        )
        .unwrap();
    assert_eq!(ids, vec![511]);
    assert_eq!(control, CanonicalScanControl::Stop);
    assert_eq!(current.get(), 0);
    assert!(!fixture.reader.is_poisoned());
}

#[test]
fn ordered_composite_warm_block_seek_reuses_full_validation_without_prefix_decode() {
    for descending in [false, true] {
        let fixture = Fixture::with_block_bytes(1024 * 1024, 1024 * 1024);
        let after = [Value::Int(100), Value::String("item-0400".into())];
        let mut reports = Vec::new();
        for _ in 0..2 {
            let mut actual = Vec::new();
            let (report, control) = fixture
                .reader
                .scan_composite_ordered_entries(
                    fixture.order(descending, Some(&after)),
                    |_| Ok(()),
                    |_, id| {
                        actual.push(id.0);
                        Ok(if actual.len() == 5 {
                            CanonicalScanControl::Stop
                        } else {
                            CanonicalScanControl::Continue
                        })
                    },
                )
                .unwrap();
            assert_eq!(control, CanonicalScanControl::Stop);
            assert_eq!(
                actual,
                if descending {
                    vec![399, 398, 397, 396, 395]
                } else {
                    vec![401, 402, 403, 404, 405]
                }
            );
            reports.push(report);
        }
        assert!(
            reports[0].entries_decoded >= 512,
            "cold validation must stay visible: {:?}",
            reports[0]
        );
        assert!(
            reports[1].entries_decoded < 32,
            "warm work must locate within the block: {:?}",
            reports[1]
        );
        assert_eq!(reports[1].cache_hits, 1);
        assert_eq!(reports[1].cache_misses, 0);
    }
}

#[test]
fn ordered_composite_verified_view_cache_refusal_remains_complete_and_healthy() {
    let fixture = Fixture::with_block_bytes(1024 * 1024, 1);
    let after = [Value::Int(100), Value::String("item-0400".into())];
    for _ in 0..2 {
        let mut ids = Vec::new();
        let (report, _) = fixture
            .reader
            .scan_composite_ordered_entries(
                fixture.order(false, Some(&after)),
                |_| Ok(()),
                |_, id| {
                    ids.push(id.0);
                    Ok(if ids.len() == 5 {
                        CanonicalScanControl::Stop
                    } else {
                        CanonicalScanControl::Continue
                    })
                },
            )
            .unwrap();
        assert_eq!(ids, vec![401, 402, 403, 404, 405]);
        assert!(report.entries_decoded >= 512);
        assert!(!fixture.reader.is_poisoned());
    }
}

fn first_block(fixture: &Fixture) -> PersistentPropertyProjectionBlockDescriptor {
    let identity = persistent_composite_property_identity(&fixture.properties).unwrap();
    let route = Route {
        order: fixture.order(false, None),
        prefix: property_projection_descriptor_prefix(
            PersistentPropertyProjectionKind::CompositeEquality,
            LabelId(1),
            &identity,
        ),
        identity,
    };
    fixture
        .reader
        .ordered_descriptor(
            &route,
            Probe::First,
            &mut PersistentPropertyProjectionReadReport::default(),
            &mut |_| Ok(()),
        )
        .unwrap()
        .unwrap()
        .0
}

#[test]
fn ordered_composite_block_proof_requires_admission_and_rechecks_selecting_metadata() {
    let fixture = Fixture::with_block_bytes(1024 * 1024, 1024 * 1024);
    let block = first_block(&fixture);
    let mut report = PersistentPropertyProjectionReadReport::default();
    let error = fixture
        .reader
        .ordered_block_view(&block, 3, &mut report, &mut |_| {
            Err::<(), _>(PersistentPropertyProjectionError::Source(
                "view workspace refused".into(),
            ))
        })
        .unwrap_err();
    assert!(matches!(
        error,
        PersistentPropertyProjectionError::Source(_)
    ));
    assert_eq!(
        report.entries_decoded, 0,
        "refuse before copying/decoding a block view"
    );
    assert!(!fixture.reader.is_poisoned());

    let mut wrong_bounds = block.clone();
    wrong_bounds.max_key = Value::List(vec![
        Value::String("note".into()),
        Value::Int(999),
        Value::String("wrong".into()),
    ]);
    let mut report = PersistentPropertyProjectionReadReport::default();
    assert!(matches!(
        fixture
            .reader
            .ordered_block_view(&wrong_bounds, 3, &mut report, &mut |_| Ok(())),
        Err(PersistentPropertyProjectionError::Corrupt(_))
    ));
    let mut report = PersistentPropertyProjectionReadReport::default();
    let _view = fixture
        .reader
        .ordered_block_view(&block, 3, &mut report, &mut |_| Ok(()))
        .unwrap();
    assert_eq!(
        report.cache_misses, 1,
        "failed validation must not publish a proof"
    );

    let mut wrong_identity = block;
    wrong_identity.label_id = LabelId(99);
    let mut report = PersistentPropertyProjectionReadReport::default();
    assert!(matches!(
        fixture
            .reader
            .ordered_block_view(&wrong_identity, 3, &mut report, &mut |_| Ok(())),
        Err(PersistentPropertyProjectionError::Corrupt(_))
    ));
}
