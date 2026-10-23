use indexmap::IndexMap;

use super::{DomHandle, DomMutationEffects};
use crate::directionality::HtmlDirection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomDirectionalityChange {
    target: DomHandle,
    before: HtmlDirection,
    after: HtmlDirection,
}

impl DomDirectionalityChange {
    pub fn target(self) -> DomHandle {
        self.target
    }
    pub fn before(self) -> HtmlDirection {
        self.before
    }
    pub fn after(self) -> HtmlDirection {
        self.after
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct DirectionalityChanges(IndexMap<DomHandle, DomDirectionalityChange>);

impl DirectionalityChanges {
    pub(super) fn merge(&mut self, other: Self) {
        for change in other.0.into_values() {
            self.record(change);
        }
    }

    fn record(&mut self, change: DomDirectionalityChange) {
        self.0
            .entry(change.target)
            .and_modify(|entry| entry.after = change.after)
            .or_insert(change);
    }
}

impl DomMutationEffects {
    /// Coalesced semantic transitions, retaining the value before the first
    /// write and after the last one. A batch that restores its original
    /// direction does not invalidate :dir() or direction presentation hints.
    pub fn directionality_changes(&self) -> impl Iterator<Item = &DomDirectionalityChange> {
        self.directionality
            .0
            .values()
            .filter(|change| change.before != change.after)
    }

    pub(in crate::native::host) fn mark_directionality_change(
        &mut self,
        target: DomHandle,
        before: HtmlDirection,
        after: HtmlDirection,
    ) {
        if before != after {
            self.directionality.record(DomDirectionalityChange {
                target,
                before,
                after,
            });
        }
    }
}
