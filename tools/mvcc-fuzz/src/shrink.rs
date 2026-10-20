// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use crate::case::{Case, Operation};
use crate::engine::{observe, Observation};

/// Preserve the failure class; invalid programs are never accepted as reductions.
pub fn shrink(case: Case, attempts: usize) -> Result<(Case, Observation, usize), String> {
    reduce_with(case, attempts, observe)
}

fn reduce_with(
    mut case: Case,
    attempts: usize,
    check: impl Fn(&Case) -> Result<Observation, String>,
) -> Result<(Case, Observation, usize), String> {
    let original = check(&case)?;
    let Some(original_failure) = original.failure.as_ref() else {
        return Err("case does not reproduce a failure".into());
    };
    if original_failure.kind == "engine_error" || original_failure.kind == "panic" {
        return Err(
            "automatic reduction requires a semantic mismatch, not an engine error or panic".into(),
        );
    }
    let mut used = 0;
    let signature = original_failure.kind.clone();
    let mut best = original;
    loop {
        let mut accepted = false;
        for candidate in candidates(&case) {
            if used == attempts {
                return Ok((case, best, used));
            }
            used += 1;
            if candidate.validate().is_err() {
                continue;
            }
            let Ok(observation) = check(&candidate) else {
                continue;
            };
            if observation
                .failure
                .as_ref()
                .is_some_and(|f| f.kind == signature)
            {
                case = candidate;
                best = observation;
                accepted = true;
                break;
            }
        }
        if !accepted {
            return Ok((case, best, used));
        }
    }
}

fn candidates(case: &Case) -> Vec<Case> {
    let mut candidates = Vec::new();
    for tx in 0..2 {
        for op in 0..case.transactions[tx].operations.len() {
            let mut candidate = case.clone();
            candidate.transactions[tx].operations.remove(op);
            if let Some(position) = candidate
                .schedule
                .iter()
                .enumerate()
                .filter(|(_, id)| **id == tx)
                .nth(op)
                .map(|(position, _)| position)
            {
                candidate.schedule.remove(position);
                candidates.push(candidate);
            }
        }
    }
    for id in case.initial.keys() {
        let mut candidate = case.clone();
        candidate.initial.remove(id);
        candidates.push(candidate);
    }
    for (id, value) in &case.initial {
        if *value != 0 {
            let mut candidate = case.clone();
            candidate.initial.insert(*id, 0);
            candidates.push(candidate);
        }
    }
    for tx in 0..2 {
        for (index, op) in case.transactions[tx].operations.iter().enumerate() {
            let mut candidate = case.clone();
            let replacement = match *op {
                Operation::Update { id, value } if value != 0 => Operation::Update { id, value: 0 },
                Operation::Insert { id, value } if value != 0 => Operation::Insert { id, value: 0 },
                Operation::BroadUpdate { minimum, value } if value != 0 => {
                    Operation::BroadUpdate { minimum, value: 0 }
                }
                _ => continue,
            };
            candidate.transactions[tx].operations[index] = replacement;
            candidates.push(candidate);
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::generate;
    use crate::engine::Failure;

    #[test]
    fn reduction_preserves_valid_schedule_and_failure_with_a_faulty_reader() {
        let case = generate(42, 0);
        let size = case.schedule.len() + case.initial.len();
        let check = |case: &Case| {
            case.validate()?;
            // A deliberately faulty reader loses every nonempty read result.
            let reads = case
                .transactions
                .iter()
                .any(|tx| tx.operations.iter().any(|op| matches!(op, Operation::Read)));
            Ok(Observation {
                trace: Vec::new(),
                failure: (reads && !case.initial.is_empty()).then(|| Failure {
                    kind: "snapshot_rows".into(),
                    step: 0,
                    detail: "faulty reader".into(),
                }),
            })
        };
        let (reduced, observation, used) = reduce_with(case, 256, check).unwrap();
        reduced.validate().unwrap();
        assert!(observation.failure.is_some());
        assert!(reduced.schedule.len() + reduced.initial.len() < size);
        assert!(used <= 256);
    }
}
