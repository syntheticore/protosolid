use std::collections::BTreeMap;

use super::PersistentId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Split,
    Merged,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Created,
    Derived {
        sources: Vec<PersistentId>,
        role: String,
    },
}

/// Complete provenance emitted by every topology-changing operation.
#[derive(Debug, Clone, Default)]
pub struct OperationHistory {
    changes: BTreeMap<PersistentId, (ChangeKind, Vec<PersistentId>)>,
    origins: BTreeMap<PersistentId, Origin>,
}

impl OperationHistory {
    pub fn changes(&self) -> &BTreeMap<PersistentId, (ChangeKind, Vec<PersistentId>)> {
        &self.changes
    }

    pub fn origins(&self) -> &BTreeMap<PersistentId, Origin> {
        &self.origins
    }
    pub fn change(&self, input: PersistentId) -> Option<(ChangeKind, &[PersistentId])> {
        self.changes
            .get(&input)
            .map(|(kind, outputs)| (*kind, outputs.as_slice()))
    }

    pub fn outputs_for(&self, input: PersistentId) -> &[PersistentId] {
        self.changes
            .get(&input)
            .map(|(_, outputs)| outputs.as_slice())
            .unwrap_or(&[])
    }

    pub fn origin(&self, output: PersistentId) -> Option<&Origin> {
        self.origins.get(&output)
    }

    pub(crate) fn created(&mut self, output: PersistentId) {
        self.origins.insert(output, Origin::Created);
    }

    pub(crate) fn derived(
        &mut self,
        output: PersistentId,
        sources: impl IntoIterator<Item = PersistentId>,
        role: impl Into<String>,
    ) {
        self.origins.insert(
            output,
            Origin::Derived {
                sources: sources.into_iter().collect(),
                role: role.into(),
            },
        );
    }

    pub(crate) fn changed(
        &mut self,
        input: PersistentId,
        kind: ChangeKind,
        outputs: impl IntoIterator<Item = PersistentId>,
    ) {
        let outputs = outputs.into_iter().collect();
        self.changes.insert(input, (kind, outputs));
    }

    pub(crate) fn rename_entity(&mut self, old: PersistentId, new: PersistentId) {
        if old == new {
            return;
        }
        if let Some(origin) = self.origins.remove(&old) {
            self.origins.insert(new, origin);
        }
        if let Some(change) = self.changes.remove(&old) {
            self.changes.insert(new, change);
        }
        for (_, outputs) in self.changes.values_mut() {
            for output in outputs {
                if *output == old {
                    *output = new;
                }
            }
        }
        for origin in self.origins.values_mut() {
            if let Origin::Derived { sources, .. } = origin {
                for source in sources {
                    if *source == old {
                        *source = new;
                    }
                }
            }
        }
    }
}
