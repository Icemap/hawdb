// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use crate::{ResourceBudgetError, ResourceBudgetKind, ResourceBudgetScope, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub(super) struct RuntimeWorkBudget {
    limit: usize,
    used: AtomicUsize,
    parent: Option<Arc<Self>>,
    refusal: Mutex<Option<ResourceBudgetError>>,
}

impl RuntimeWorkBudget {
    pub(super) fn new(limit: usize, parent: Option<Arc<Self>>) -> Self {
        Self {
            limit,
            used: AtomicUsize::new(0),
            parent,
            refusal: Mutex::new(None),
        }
    }

    pub(super) fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub(super) fn refusal(&self) -> Option<ResourceBudgetError> {
        self.refusal
            .lock()
            .expect("work refusal lock")
            .clone()
            .or_else(|| self.parent.as_ref().and_then(|parent| parent.refusal()))
    }

    pub(super) fn charge(&self, units: usize) -> Result<()> {
        // A parent's refusal may leave a conservative child charge, never an
        // undercharge. Query failure discards the complete scoped result.
        self.used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(units).filter(|next| *next <= self.limit)
            })
            .map_err(|used| {
                let error = ResourceBudgetError {
                    resource: ResourceBudgetKind::WorkUnits,
                    scope: ResourceBudgetScope::Operator,
                    owner: "bounded query input".into(),
                    memory_class: None,
                    requested: units as u64,
                    used: used as u64,
                    budget: self.limit as u64,
                    budget_option: "RuntimeTaskContext.work_limit",
                    message: "query input exceeds its work limit; no partial domain is returned"
                        .into(),
                };
                let mut refusal = self.refusal.lock().expect("work refusal lock");
                if refusal.is_none() {
                    *refusal = Some(error.clone());
                }
                error
            })?;
        if let Some(parent) = &self.parent {
            parent.charge(units)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{HawDBError, RuntimeTaskContext};

    #[test]
    fn shared_work_limit_preserves_parent_and_failed_charge() {
        let parent = RuntimeTaskContext::default().with_work_limit(3);
        let child = parent.clone().with_work_limit(2);
        child.child().charge_work(2).unwrap();
        assert_eq!(child.work_units(), 2);
        assert_eq!(parent.work_units(), 2);
        assert!(matches!(
            child.charge_work(1),
            Err(HawDBError::ResourceBudgetExceeded(_))
        ));
        assert_eq!(parent.work_units(), 2);
        assert!(matches!(
            child.check_work_limit(),
            Err(HawDBError::ResourceBudgetExceeded(_))
        ));
        assert!(parent.check_work_limit().is_ok());
        parent.charge_work(1).unwrap();
        let another = parent.clone().with_work_limit(10);
        assert!(another.charge_work(1).is_err());
        assert_eq!(parent.work_units(), 3);
    }

    #[test]
    fn work_charge_observes_original_cancellation() {
        let task = RuntimeTaskContext::default().with_work_limit(1);
        let child = task.child();
        task.cancellation().cancel();
        assert!(child.charge_work(1).is_err());
        assert_eq!(task.work_units(), 0);
    }
}
