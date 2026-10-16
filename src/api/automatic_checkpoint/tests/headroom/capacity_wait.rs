use super::*;

#[test]
fn critical_preparation_wait_returns_busy_without_resetting_its_deadline() {
    let fixture = Fixture::new();
    let (db, _suspension, _) = seeded(&fixture);
    let control = Arc::new(Control::default());
    {
        let mut state = control.lock().unwrap();
        state.phase = Phase::Preparing;
        let mut debt = db
            .runtime
            .peek()
            .unwrap()
            .store
            .checkpoint_debt_snapshot()
            .unwrap();
        debt.delta_bytes = debt.max_delta_bytes.unwrap() * 85 / 100;
        state.debt = Some(debt);
        assert!(preparation_needs_capacity(&state));
    }
    let notifying = control.clone();
    let notifier = std::thread::spawn(move || {
        let deadline = Instant::now() + PREPARATION_CAPACITY_WAIT_LIMIT * 2;
        while Instant::now() < deadline {
            notifying.changed.notify_all();
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let started = Instant::now();
    assert!(matches!(
        control.lock_frontend(),
        Err(HawDBError::BranchBusy {
            resource: "automatic checkpoint capacity"
        })
    ));
    let elapsed = started.elapsed();
    notifier.join().unwrap();
    assert!(elapsed >= PREPARATION_CAPACITY_WAIT_LIMIT);
    assert!(elapsed < PREPARATION_CAPACITY_WAIT_LIMIT * 2);
    assert_eq!(control.lock().unwrap().phase, Phase::Preparing);
    control.ensure_healthy().unwrap();
}

#[test]
fn parked_preparation_rejects_no_data_and_a_busy_writer_can_retry_after_checkpoint() {
    let fixture = Fixture::new();
    let (mut db, suspension, mut next) = seeded(&fixture);
    while !preparation_needs_capacity(&db.runtime.checkpoint_control_for_test().lock().unwrap()) {
        create(&mut db, next).unwrap();
        next += 1;
    }
    let control = db.runtime.checkpoint_control_for_test();
    let (sealed, observed) = mpsc::channel();
    let (resume, continuation) = mpsc::channel();
    control.lock().unwrap().prefix_seal_probe = Some(Arc::new(PrefixSealProbe {
        sealed,
        resume: Mutex::new(continuation),
    }));
    drop(suspension);
    observed.recv_timeout(Duration::from_secs(15)).unwrap();
    let before = db
        .runtime
        .peek()
        .unwrap()
        .store
        .checkpoint_source_identity();
    let result = create(&mut db, next);
    let after = db
        .runtime
        .peek()
        .unwrap()
        .store
        .checkpoint_source_identity();
    // Always release the test-only owner pause before checking the outcome.
    control.lock().unwrap().prefix_seal_probe = None;
    resume.send(()).unwrap();
    assert!(matches!(
        result,
        Err(HawDBError::BranchBusy {
            resource: "automatic checkpoint capacity"
        })
    ));
    assert_eq!(after, before);
    db.checkpoint().unwrap();
    create(&mut db, next).unwrap();
    let expected = db
        .query("MATCH (n:Memory) RETURN n.id AS id, n.body AS body ORDER BY id")
        .unwrap()
        .rows;
    assert_eq!(expected.len(), usize::try_from(next + 1).unwrap());
    for (id, row) in expected.iter().enumerate() {
        assert_eq!(
            row.get("id"),
            Some(&crate::Value::Int(i64::try_from(id).unwrap()))
        );
        assert_eq!(
            row.get("body"),
            Some(&crate::Value::String("b".repeat(512)))
        );
    }
    drop(db);
    let mut reopened = Database::open(&fixture.0).unwrap();
    assert_eq!(
        reopened
            .query("MATCH (n:Memory) RETURN n.id AS id, n.body AS body ORDER BY id")
            .unwrap()
            .rows,
        expected
    );
}

fn releasing_control(delta_bytes: u64) -> Arc<Control> {
    let control = Arc::new(Control::default());
    {
        let mut state = control.lock().unwrap();
        state.phase = Phase::Releasing;
        state.debt = Some(CheckpointDebtSnapshot {
            commit_epoch: 1,
            checkpoint_commit_epoch: 0,
            wal_generation: 1,
            wal_bytes: 0,
            wal_age_millis: 0,
            max_wal_bytes: None,
            max_wal_record_bytes: None,
            delta_bytes,
            max_delta_bytes: Some(16 * 1024),
            read_only: false,
            wal_sync_group_active: false,
        });
    }
    control
}

#[test]
fn releasing_old_owners_preserves_available_write_capacity() {
    let control = releasing_control(16 * 1024 * 70 / 100);
    let state = control.lock_frontend().unwrap();
    assert_eq!(state.phase, Phase::Releasing);
}

#[test]
fn releasing_old_owners_fences_capacity_growth_until_cleanup_finishes() {
    let control = releasing_control(16 * 1024 * 85 / 100);
    let (waiting, observed) = mpsc::channel();
    control.lock().unwrap().frontend_wait_probe = Some(waiting);
    let (finished, completion) = mpsc::channel();
    let writer_control = control.clone();
    let writer = std::thread::spawn(move || {
        let phase = writer_control.lock_frontend().unwrap().phase;
        finished.send(phase).unwrap();
    });
    let phase = observed.recv_timeout(Duration::from_secs(1));
    let early = completion.try_recv();
    {
        let mut state = control.lock().unwrap();
        state.phase = Phase::Idle;
        control.notify_all();
    }
    writer.join().unwrap();
    assert_eq!(phase.unwrap(), Phase::Releasing);
    assert!(matches!(early, Err(mpsc::TryRecvError::Empty)));
    assert_eq!(completion.recv().unwrap(), Phase::Idle);
}
