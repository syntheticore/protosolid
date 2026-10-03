use std::collections::{BTreeMap, BTreeSet};

use super::{PersistentId, Topology};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// The operation retains the input revision; tolerance-sized edits do not qualify.
    Unchanged,
    Modified,
    Split,
    Merged,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Created {
        role: Option<CreationRole>,
    },
    Derived {
        sources: Vec<PersistentId>,
        role: String,
    },
}

/// Stable semantic role of topology created directly by a high-level operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreationRole {
    ExtrusionStart,
    ExtrusionEnd,
    ExtrusionSide {
        boundary: ProfileBoundary,
        segment: usize,
        part: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileBoundary {
    Outer,
    Hole(usize),
}

/// Complete provenance emitted by every topology-changing operation.
#[derive(Debug, Clone, Default)]
pub struct OperationHistory {
    changes: BTreeMap<PersistentId, (ChangeKind, Vec<PersistentId>)>,
    origins: BTreeMap<PersistentId, Origin>,
    // Immediate correspondence supplied by construction, including retention.
    pub(crate) construction: BTreeMap<PersistentId, Vec<EntitySource>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntitySource {
    pub name: PersistentId,
    pub unchanged: bool,
}

impl EntitySource {
    pub fn preserved(name: PersistentId) -> Self {
        Self {
            name,
            unchanged: true,
        }
    }
}

impl OperationHistory {
    /// Fill untouched/deleted identities; explicit operator edits take precedence.
    pub(crate) fn complete_retained(&mut self, before: &Topology, after: &Topology) {
        fn names(topology: &Topology) -> impl Iterator<Item = PersistentId> + '_ {
            topology
                .vertices()
                .map(|(_, value)| value.name())
                .chain(topology.edges().map(|(_, value)| value.name()))
                .chain(topology.coedges().map(|(_, value)| value.name()))
                .chain(topology.loops().map(|(_, value)| value.name()))
                .chain(topology.faces().map(|(_, value)| value.name()))
                .chain(topology.shells().map(|(_, value)| value.name()))
                .chain(topology.bodies().map(|(_, value)| value.name()))
        }
        let outputs = names(after).collect::<BTreeSet<_>>();
        for source in names(before) {
            self.changes.entry(source).or_insert_with(|| {
                if outputs.contains(&source) {
                    (ChangeKind::Unchanged, vec![source])
                } else {
                    (ChangeKind::Deleted, Vec::new())
                }
            });
        }
    }

    pub(crate) fn identity(&self, names: impl IntoIterator<Item = PersistentId>) -> Self {
        Self {
            changes: names
                .into_iter()
                .map(|name| (name, (ChangeKind::Unchanged, vec![name])))
                .collect(),
            origins: self.origins.clone(),
            construction: self.construction.clone(),
        }
    }

    pub(crate) fn source_roots(&self, entity: PersistentId) -> BTreeSet<PersistentId> {
        let mut pending = vec![entity];
        let mut visited = BTreeSet::new();
        let mut roots = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !visited.insert(current) {
                continue;
            }
            match self.origin(current) {
                Some(Origin::Derived { sources, .. }) if !sources.is_empty() => {
                    pending.extend(sources.iter().copied());
                }
                _ => {
                    roots.insert(current);
                }
            }
        }
        if roots.is_empty() {
            visited
        } else {
            roots
        }
    }

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
        self.origins.insert(output, Origin::Created { role: None });
    }

    pub(crate) fn created_as(&mut self, output: PersistentId, role: CreationRole) {
        self.origins
            .insert(output, Origin::Created { role: Some(role) });
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
        if let Some(records) = self.construction.remove(&old) {
            self.construction.insert(new, records);
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

    pub(crate) fn absorb(&mut self, other: Self) {
        self.changes.extend(other.changes);
        self.origins.extend(other.origins);
        self.construction.extend(other.construction);
    }

    pub(crate) fn forget_output(&mut self, name: PersistentId) {
        self.origins.remove(&name);
        self.construction.remove(&name);
    }

    pub(crate) fn record_construction(
        &mut self,
        output: PersistentId,
        sources: impl IntoIterator<Item = EntitySource>,
    ) {
        let records = self.construction.entry(output).or_default();
        for source in sources {
            if let Some(existing) = records.iter_mut().find(|record| record.name == source.name) {
                existing.unchanged |= source.unchanged;
            } else {
                records.push(source);
            }
        }
    }

    pub(crate) fn merge_construction(&mut self, removed: PersistentId, survivor: PersistentId) {
        let removed = self.construction.remove(&removed).unwrap_or_default();
        self.record_construction(survivor, removed);
        if let Some(records) = self.construction.get_mut(&survivor) {
            for source in records {
                source.unchanged = false;
            }
        }
    }

    pub(crate) fn modified(&mut self, entity: PersistentId) {
        if matches!(self.change(entity), None | Some((ChangeKind::Unchanged, _))) {
            self.changed(entity, ChangeKind::Modified, [entity]);
        }
    }
}
