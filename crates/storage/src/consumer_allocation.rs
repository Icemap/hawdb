// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Durable, bounded reservations independent of the rebuildable Consumer cache.
//! Reservations are not directory birth or publication receipts. Pending records
//! remain until a later native-custody protocol can settle their actual effects.

use crate::branch_catalog::BranchId;
use crate::branch_project::{CapturedProject, ProjectSelector};
use crate::file_descriptors::FileOpenContext;
use crate::file_io::{File, SourceDirectory};
use crate::ownership::DatabaseDirectoryLease;
use hawdb_core::{HawDBError, Result, Uuid};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;

pub(crate) const LEDGER_FILE: &str = "consumer-allocations.hawdb";
const MAX_BYTES: usize = 1024 * 1024;
const MAX_PENDING: usize = 256;
const HEADER: &[u8] = b"HAWDB_CONSUMER_ALLOCATIONS_V1\n";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    project_id: String,
    main_branch_id: String,
    high_water: u64,
    pending: Vec<Reservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reservation {
    branch_id: String,
    database_uuid: String,
    consumer_id: String,
    target_locator: String,
    target_incarnation: u64,
    generation: u64,
    operation: u64,
    registration_uuid: String,
    projection_uuid: String,
    checkpoint_uuid: String,
}

#[derive(Debug)]
struct TargetIncarnation(u64);
#[derive(Debug)]
struct ImmutableGeneration(u64);
#[derive(Debug)]
struct OperationIdentity(u64);

/// Kernel-owned source admission. It checks initialized allocation evidence
/// before identity/checkpoint changes, and retains original custody for reserve.
#[doc(hidden)]
#[derive(Debug)]
pub(crate) struct ConsumerAllocationSource {
    project: CapturedProject,
    owner: Arc<DatabaseDirectoryLease>,
    branch: BranchId,
}
impl ConsumerAllocationSource {
    pub(crate) fn reserve(
        &self,
        database: Uuid,
        consumer: &str,
        destination: &Path,
    ) -> Result<ReservedConsumerAllocation> {
        reserve(
            &self.project,
            Arc::clone(&self.owner),
            self.branch,
            database,
            consumer,
            destination,
        )
    }
}

pub(crate) fn source(
    project: &CapturedProject,
    owner: Arc<DatabaseDirectoryLease>,
    branch: BranchId,
) -> Result<ConsumerAllocationSource> {
    project.with_locked_catalog(|root, context, catalog| {
        check_source(project, &owner, branch, root, context, &catalog).map(|_| ())
    })?;
    Ok(ConsumerAllocationSource {
        project: project.clone(),
        owner,
        branch,
    })
}

fn check_source(
    project: &CapturedProject,
    owner: &DatabaseDirectoryLease,
    branch: BranchId,
    root: &SourceDirectory,
    context: &FileOpenContext,
    catalog: &crate::branch_catalog::Catalog,
) -> Result<Ledger> {
    let record = catalog
        .branches
        .iter()
        .find(|r| r.id == branch && r.state == crate::branch_catalog::BranchState::Ready)
        .ok_or_else(|| invalid("source branch is not ready"))?;
    let actual = project.capture_branch_directory(record.id)?;
    if !actual.same_directory(owner.captured_directory()) {
        return Err(invalid("source owner differs from original project"));
    }
    let ledger = read(root, context)?;
    if selector(&ledger)? != project.allocation_identity()
        || catalog.project_id != project.allocation_identity().project_id()
    {
        return Err(invalid("project binding differs"));
    }
    Ok(ledger)
}

/// Kernel-created reservation, never a native birth/adoption/deletion token.
#[doc(hidden)]
#[derive(Debug)]
pub struct ReservedConsumerAllocation {
    domain: BranchId,
    incarnation: TargetIncarnation,
    generation: ImmutableGeneration,
    operation: OperationIdentity,
    registration: Uuid,
    projection: Uuid,
    checkpoint: Uuid,
    _project: CapturedProject,
    _source: Arc<DatabaseDirectoryLease>,
}

impl ReservedConsumerAllocation {
    pub fn registration_uuid(&self) -> Uuid {
        self.registration
    }
    pub fn projection_uuid(&self) -> Uuid {
        self.projection
    }
    pub fn checkpoint_uuid(&self) -> Uuid {
        self.checkpoint
    }
    pub fn stage_name(&self) -> String {
        format!(
            ".hawdb-consumer-{}-{}-{}-{}.stage",
            self.domain.as_uuid(),
            self.incarnation.0,
            self.generation.0,
            self.operation.0
        )
    }
}

fn invalid(message: &str) -> HawDBError {
    HawDBError::StorageIntegrity(format!("consumer allocation ledger: {message}"))
}

fn read(root: &SourceDirectory, context: &FileOpenContext) -> Result<Ledger> {
    let file = match root.open_child(context, LEDGER_FILE) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(invalid("initialized domain ledger is missing"))
        }
        Err(error) => return Err(error.into()),
    };
    decode(file)
}

fn decode(file: File) -> Result<Ledger> {
    if file.metadata()?.len() > MAX_BYTES as u64 {
        return Err(invalid("byte limit exceeded"));
    }
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES || !bytes.starts_with(HEADER) || bytes.len() < HEADER.len() + 4 {
        return Err(invalid("missing or incomplete header"));
    }
    let checksum_at = bytes.len() - 4;
    let checksum = u32::from_le_bytes(
        bytes[checksum_at..]
            .try_into()
            .expect("four checksum bytes"),
    );
    if hawdb_integrity::crc32c(&bytes[..checksum_at]).get() != checksum {
        return Err(invalid("checksum mismatch"));
    }
    let ledger: Ledger = serde_json::from_slice(&bytes[HEADER.len()..checksum_at])
        .map_err(|_| invalid("invalid payload"))?;
    validate(&ledger)?;
    if encode(&ledger)? != bytes {
        return Err(invalid("noncanonical payload"));
    }
    Ok(ledger)
}

fn validate(ledger: &Ledger) -> Result<()> {
    let project =
        BranchId::parse(&ledger.project_id).map_err(|_| invalid("invalid project identity"))?;
    let main =
        BranchId::parse(&ledger.main_branch_id).map_err(|_| invalid("invalid main identity"))?;
    ProjectSelector::new(project, main)?;
    if ledger.pending.len() > MAX_PENDING || !ledger.high_water.is_multiple_of(3) {
        return Err(invalid("invalid allocator boundary"));
    }
    let mut previous = 0;
    for row in &ledger.pending {
        if row.operation > ledger.high_water
            || row.operation <= previous
            || !row.operation.is_multiple_of(3)
            || row.operation.checked_sub(2) != Some(row.target_incarnation)
            || row.operation.checked_sub(1) != Some(row.generation)
            || row.target_incarnation == 0
        {
            return Err(invalid("invalid reservation identities"));
        }
        BranchId::parse(&row.branch_id).map_err(|_| invalid("invalid source branch"))?;
        for id in [
            &row.database_uuid,
            &row.registration_uuid,
            &row.projection_uuid,
            &row.checkpoint_uuid,
        ] {
            let id = Uuid::parse_str(id).map_err(|_| invalid("invalid binding UUID"))?;
            if id.is_nil() {
                return Err(invalid("nil binding UUID"));
            }
        }
        if row.consumer_id.is_empty()
            || row.consumer_id.len() > 128
            || !row
                .consumer_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || row.target_locator.len() > 16 * 1024
        {
            return Err(invalid("invalid consumer or target locator"));
        }
        previous = row.operation;
    }
    Ok(())
}

struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .0
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= MAX_BYTES - 4)
            .ok_or_else(|| io::Error::other("consumer allocation ledger byte limit exceeded"))?;
        self.0
            .try_reserve_exact(next - self.0.len())
            .map_err(io::Error::other)?;
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(ledger: &Ledger) -> Result<Vec<u8>> {
    let mut bytes = BoundedBytes(Vec::new());
    bytes.write_all(HEADER)?;
    serde_json::to_writer(&mut bytes, ledger)
        .map_err(|_| invalid("encoding byte limit exceeded"))?;
    let checksum = hawdb_integrity::crc32c(&bytes.0);
    bytes.0.extend_from_slice(&checksum.get().to_le_bytes());
    Ok(bytes.0)
}
fn selector(ledger: &Ledger) -> Result<ProjectSelector> {
    ProjectSelector::new(
        BranchId::parse(&ledger.project_id).map_err(|_| invalid("invalid project"))?,
        BranchId::parse(&ledger.main_branch_id).map_err(|_| invalid("invalid main"))?,
    )
}

/// Caller owns original-root bootstrap and original catalog exclusion. No source
/// identity may be published, reservation exposed or target effect run before
/// this complete file and its parent namespace have synchronized.
pub(crate) fn bootstrap(
    root: &SourceDirectory,
    context: &FileOpenContext,
    proposed: ProjectSelector,
    virgin: bool,
) -> Result<ProjectSelector> {
    match root.open_mutable_child(context, LEDGER_FILE, false) {
        Ok(file) => {
            // A prior attempt may have written complete bytes but lost its
            // file barrier. Retry that barrier through this exact native file
            // before publishing any initialized source identity.
            file.sync_all()?;
            let ledger = decode(file)?;
            if virgin && (ledger.high_water != 0 || !ledger.pending.is_empty()) {
                return Err(invalid(
                    "uninitialized source has nonempty allocation evidence",
                ));
            }
            let selected = selector(&ledger)?;
            if !virgin && selected != proposed {
                return Err(invalid("source identity differs"));
            }
            // Retry the namespace barrier even after a lost bootstrap response.
            root.sync(context)?;
            Ok(selected)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound && virgin => {
            let ledger = Ledger {
                project_id: proposed.project_id().as_uuid().to_string(),
                main_branch_id: proposed.main_branch_id().as_uuid().to_string(),
                high_water: 0,
                pending: Vec::new(),
            };
            let bytes = encode(&ledger)?;
            let mut file = root.create_child_new(context, LEDGER_FILE)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            root.sync(context)?;
            Ok(proposed)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(invalid("initialized domain ledger is missing"))
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn reserve(
    project: &CapturedProject,
    owner: Arc<DatabaseDirectoryLease>,
    branch: BranchId,
    database: Uuid,
    consumer: &str,
    destination: &Path,
) -> Result<ReservedConsumerAllocation> {
    let locator = destination
        .to_str()
        .filter(|s| s.len() <= 16 * 1024)
        .ok_or_else(|| invalid("target locator must be bounded UTF-8"))?;
    let registration = hawdb_core::generate_uuidv7()?;
    let projection = hawdb_core::generate_uuidv7()?;
    let checkpoint = hawdb_core::generate_uuidv7()?;
    let operation = project.with_locked_catalog(|root, context, catalog| {
        let mut ledger = check_source(project, &owner, branch, root, context, &catalog)?;
        if ledger.pending.len() >= MAX_PENDING {
            return Err(invalid("pending reservation budget exhausted"));
        }
        let operation = ledger
            .high_water
            .checked_add(3)
            .ok_or_else(|| invalid("allocation identities exhausted"))?;
        ledger.pending.push(Reservation {
            branch_id: branch.as_uuid().to_string(),
            database_uuid: database.to_string(),
            consumer_id: consumer.into(),
            target_locator: locator.into(),
            target_incarnation: operation - 2,
            generation: operation - 1,
            operation,
            registration_uuid: registration.to_string(),
            projection_uuid: projection.to_string(),
            checkpoint_uuid: checkpoint.to_string(),
        });
        ledger.high_water = operation;
        validate(&ledger)?;
        let bytes = encode(&ledger)?;
        let name = format!("consumer-allocation-{}.tmp", hawdb_core::generate_uuidv7()?);
        let mut candidate = root.create_control_candidate(context, &name)?;
        candidate.write_all(&bytes)?;
        candidate.seal()?;
        candidate.replace_head(context, LEDGER_FILE)?;
        Ok(operation)
    })?;
    Ok(ReservedConsumerAllocation {
        domain: project.allocation_identity().project_id(),
        incarnation: TargetIncarnation(operation - 2),
        generation: ImmutableGeneration(operation - 1),
        operation: OperationIdentity(operation),
        registration,
        projection,
        checkpoint,
        _project: project.clone(),
        _source: owner,
    })
}
