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

//! Keep reference runs, decoded children, and completed pages across denials.
use super::*;

#[derive(Default)]
pub(super) struct Progress {
    phase: u8,
    current: Option<RefRun>,
    level: Option<Level>,
    height: u32,
    root: Option<GraphDescriptorPageRef>,
    prepared: Option<PreparedGraphDescriptorTree>,
}

struct Level {
    reader: RefRunReader,
    writer: RefRunWriter,
    children: Vec<GraphDescriptorInteriorEntry>,
    payload_bytes: usize,
    child: Option<GraphDescriptorPageRef>,
    child_bytes: Option<usize>,
    page: Option<PendingLeafPage>,
    eof: bool,
    reader_finished: bool,
    next: Option<RefRun>,
}

pub(super) fn resume(
    builder: &mut GraphDescriptorTreeBuilder,
    progress: &mut Progress,
) -> Result<PreparedGraphDescriptorTree, GraphDescriptorTreeError> {
    loop {
        builder.work.checkpoint()?;
        match progress.phase {
            0 => {
                builder.flush_leaf_page()?;
                progress.phase = 1;
            }
            1 => {
                let unit = builder.work.start_unit()?;
                {
                    let _wave = builder.work.io_wave()?;
                    let run = builder
                        .level_zero
                        .as_mut()
                        .expect("level-zero writer")
                        .finish_retained()?;
                    builder.record_finished_run(&run)?;
                    progress.current = Some(run);
                    builder.level_zero = None;
                    progress.phase = 2;
                }
                unit.finish();
            }
            2 => {
                let current = progress.current.as_ref().expect("current reference level");
                if current.count <= 1 {
                    let unit = builder.work.start_unit()?;
                    {
                        let _wave = builder.work.io_wave()?;
                        if current.count == 1 {
                            let mut reader = RefRunReader::open(&current.path, 1, builder.config)?;
                            let root = reader.next_ref()?.ok_or_else(|| {
                                corrupt("graph descriptor root reference run is empty")
                            })?;
                            reader.finish()?;
                            progress.root = Some(root);
                        }
                        builder.remove_temporary(&current.path)?;
                        progress.current = None;
                        progress.phase = 3;
                    }
                    unit.finish();
                } else {
                    build_level(builder, progress)?;
                }
            }
            3 => {
                let unit = builder.work.start_unit()?;
                {
                    let _wave = builder.work.io_wave()?;
                    builder.page_writer.flush()?;
                    builder.page_writer.get_ref().sync_all()?;
                }
                prepare(builder, progress)?;
                progress.phase = 4;
                unit.finish();
            }
            4 => {
                builder.work.checkpoint()?;
                let mut prepared = progress.prepared.take().expect("completed descriptor root");
                prepared.work = builder.work.clone();
                builder.temporary_files.disarm(&prepared.page_tmp);
                return Ok(prepared);
            }
            _ => unreachable!("private descriptor finish phase"),
        }
    }
}

fn build_level(
    builder: &mut GraphDescriptorTreeBuilder,
    progress: &mut Progress,
) -> Result<(), GraphDescriptorTreeError> {
    if progress.level.is_none() {
        let unit = builder.work.start_unit()?;
        let height = progress
            .height
            .checked_add(1)
            .ok_or_else(|| admission("graph descriptor tree height overflow"))?;
        let next_path = builder.paths.ref_run(height);
        let required = builder
            .total_intermediate_bytes
            .checked_add(REF_RUN_MAGIC.len() as u64)
            .ok_or_else(|| admission("graph descriptor intermediate byte count overflow"))?;
        if required > builder.config.max_intermediate_bytes.get() {
            return Err(admission(format!(
                "graph descriptor build requires {required} intermediate bytes, exceeding limit {}",
                builder.config.max_intermediate_bytes
            )));
        }
        {
            let _wave = builder.work.io_wave()?;
            remove_if_exists(&next_path)?;
            builder.temporary_files.track(next_path.clone());
            let writer = if builder.checkpoint_buffers {
                RefRunWriter::create_checkpoint(next_path, &builder.work)?
            } else {
                RefRunWriter::create(next_path)?
            };
            let current = progress.current.as_ref().expect("current reference level");
            let reader = RefRunReader::open(&current.path, current.count, builder.config)?;
            progress.level = Some(Level {
                reader,
                writer,
                children: Vec::new(),
                payload_bytes: 0,
                child: None,
                child_bytes: None,
                page: None,
                eof: false,
                reader_finished: false,
                next: None,
            });
            progress.height = height;
            builder.total_intermediate_bytes = required;
        }
        unit.finish();
        builder.work.checkpoint()?;
    }
    let level = progress.level.as_mut().expect("active reference level");
    if level.page.is_some() {
        flush_page(builder, level)?;
        return Ok(());
    }
    if !level.eof && level.child.is_none() {
        let unit = builder.work.start_unit()?;
        {
            let _wave = builder.work.io_wave()?;
            level.child = level.reader.next_ref()?;
            level.eof = level.child.is_none();
        }
        unit.finish();
        return Ok(());
    }
    if let Some(child) = level.child.as_ref() {
        if level.child_bytes.is_none() {
            let unit = builder.work.start_unit()?;
            let bytes = interior_entry_payload_bytes(child)?;
            let limit = builder
                .config
                .page_limits
                .max_payload_bytes()
                .expect("validated page limit");
            if bytes > limit {
                return Err(admission(format!("graph descriptor interior entry requires {bytes} payload bytes, exceeding page payload limit {limit}")));
            }
            level.child_bytes = Some(bytes);
            unit.finish();
            return Ok(());
        }
        let bytes = level.child_bytes.expect("measured child");
        let limit = builder
            .config
            .page_limits
            .max_payload_bytes()
            .expect("validated page limit");
        let full = !level.children.is_empty()
            && (level.children.len() == builder.config.page_limits.max_entries.get()
                || level
                    .payload_bytes
                    .checked_add(bytes)
                    .is_none_or(|n| n > limit));
        if full {
            flush_page(builder, level)?;
            return Ok(());
        }
        let unit = builder.work.start_unit()?;
        level.payload_bytes = level
            .payload_bytes
            .checked_add(bytes)
            .ok_or_else(|| admission("graph descriptor interior payload overflow"))?;
        level.children.push(GraphDescriptorInteriorEntry {
            child: level.child.take().expect("captured child"),
        });
        level.child_bytes = None;
        builder.observe_resident_interior(level.payload_bytes, &level.children);
        unit.finish();
        return Ok(());
    }
    if !level.children.is_empty() {
        flush_page(builder, level)?;
        return Ok(());
    }
    if !level.reader_finished {
        let unit = builder.work.start_unit()?;
        {
            let _wave = builder.work.io_wave()?;
            level.reader.finish()?;
            level.reader_finished = true;
        }
        unit.finish();
        return Ok(());
    }
    if level.next.is_none() {
        let unit = builder.work.start_unit()?;
        {
            let _wave = builder.work.io_wave()?;
            let next = level.writer.finish_retained()?;
            builder.record_finished_run(&next)?;
            level.next = Some(next);
        }
        unit.finish();
        return Ok(());
    }
    let unit = builder.work.start_unit()?;
    {
        let _wave = builder.work.io_wave()?;
        builder.remove_temporary(
            &progress
                .current
                .as_ref()
                .expect("previous reference level")
                .path,
        )?;
        progress.current = level.next.take();
        progress.level = None;
    }
    unit.finish();
    Ok(())
}

fn flush_page(
    builder: &mut GraphDescriptorTreeBuilder,
    level: &mut Level,
) -> Result<(), GraphDescriptorTreeError> {
    if level.page.is_none() {
        let page_id = builder.allocate_page_id()?;
        level.page = Some(PendingLeafPage {
            page: ImmutableGraphDescriptorPage {
                kind: builder.kind,
                physical_generation: builder.generation,
                source_commit_epoch: builder.source_commit_epoch,
                page_id,
                body: ImmutableGraphDescriptorPageBody::Interior(std::mem::take(
                    &mut level.children,
                )),
            },
            reference: None,
        });
    }
    let page = level.page.as_mut().expect("captured interior page");
    if page.reference.is_none() {
        page.reference = Some(builder.write_page(&page.page)?);
    }
    builder.write_ref(
        &mut level.writer,
        page.reference.as_ref().expect("written interior page"),
    )?;
    level.page = None;
    level.payload_bytes = 0;
    Ok(())
}

fn prepare(
    builder: &mut GraphDescriptorTreeBuilder,
    progress: &mut Progress,
) -> Result<(), GraphDescriptorTreeError> {
    let page_integrity = builder.page_hasher.clone().finish();
    let root_manifest = GraphDescriptorTreeRoot {
        kind: builder.kind,
        generation: builder.generation,
        source_commit_epoch: builder.source_commit_epoch,
        page_artifact_id: builder.artifact_id,
        page_artifact_len: builder.page_artifact_bytes,
        page_artifact_crc32c: page_integrity.crc32c,
        page_artifact_sha256: page_integrity.sha256,
        descriptor_count: builder.descriptor_count,
        page_count: builder.page_count,
        leaf_page_count: builder.leaf_page_count,
        height: progress.height,
        root: progress.root.take(),
    };
    let encoded_root = root_manifest.encode(builder.config)?;
    builder.peak_resident_bytes = builder.peak_resident_bytes.max(
        TREE_IO_BUFFER_BYTES
            .saturating_add(encoded_root.len())
            .saturating_add(
                root_manifest
                    .root
                    .as_ref()
                    .map_or(0, page_ref_resident_bytes),
            ) as u64,
    );
    let report = GraphDescriptorTreeBuildReport {
        descriptor_count: builder.descriptor_count,
        page_count: builder.page_count,
        leaf_page_count: builder.leaf_page_count,
        interior_page_count: builder.page_count.saturating_sub(builder.leaf_page_count),
        page_artifact_bytes: builder.page_artifact_bytes,
        root_bytes: encoded_root.len() as u64,
        total_intermediate_bytes: builder.total_intermediate_bytes,
        peak_intermediate_level_bytes: builder.peak_intermediate_level_bytes,
        peak_resident_bytes: builder.peak_resident_bytes,
    };
    let page_tmp = builder.paths.page_tmp();
    progress.prepared = Some(PreparedGraphDescriptorTree {
        work: builder.work.clone(),
        paths: builder.paths.clone(),
        config: builder.config,
        root: root_manifest,
        encoded_root,
        report,
        page_tmp,
        published: false,
    });
    Ok(())
}
