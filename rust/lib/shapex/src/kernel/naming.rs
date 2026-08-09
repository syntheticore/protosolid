use std::collections::{BTreeMap, BTreeSet};

use uuid::Uuid;

use super::{DiagnosticKind, KernelError, KernelResult};

/// Stable identity of a topological entity across modeling operations.
///
/// The kernel generates the namespace and assigns deterministic local values
/// according to Euler-operation roles and ordinals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PersistentId {
    pub namespace: Uuid,
    pub local: u64,
}

impl PersistentId {
    pub(crate) const fn new(namespace: Uuid, local: u64) -> Self {
        Self { namespace, local }
    }
}

/// Deterministic name allocator scoped to one operation.
#[derive(Debug, Clone)]
pub(crate) struct NameSource {
    namespace: Uuid,
    role_ordinals: BTreeMap<String, u64>,
    allocated: BTreeSet<u64>,
    scope: Vec<String>,
}

impl NameSource {
    pub fn new(namespace: Uuid) -> Self {
        Self {
            namespace,
            role_ordinals: BTreeMap::new(),
            allocated: BTreeSet::new(),
            scope: Vec::new(),
        }
    }

    /// Allocate from a deterministic role-specific stream.
    ///
    /// Adding a new role to an algorithm does not renumber existing roles. The
    /// ordinal only advances within that role, preserving stable names for
    /// unaffected output categories during feature evolution.
    pub fn next_for(&mut self, role: &str) -> KernelResult<PersistentId> {
        if role.is_empty() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "persistent naming roles may not be empty",
            ));
        }
        let qualified_role = if self.scope.is_empty() {
            role.to_owned()
        } else {
            let encoded_scope = self
                .scope
                .iter()
                .map(|component| format!("{}:{component}", component.len()))
                .collect::<Vec<_>>()
                .join("/");
            format!("{encoded_scope}/{role}")
        };
        let ordinal = self
            .role_ordinals
            .entry(qualified_role.clone())
            .or_insert(0);
        let local = role_local_id(&qualified_role, *ordinal);
        *ordinal = ordinal.checked_add(1).ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::NumericalFailure,
                "persistent naming role exhausted its ordinals",
            )
        })?;
        if !self.allocated.insert(local) {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "persistent naming hash collision",
            ));
        }
        Ok(PersistentId::new(self.namespace, local))
    }

    /// Allocate a name from a semantic key rather than a positional ordinal.
    pub fn named(&mut self, role: &str, key: &str) -> KernelResult<PersistentId> {
        if role.is_empty() || key.is_empty() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "persistent naming roles and semantic keys may not be empty",
            ));
        }
        let encoded_scope = self
            .scope
            .iter()
            .map(|component| format!("{}:{component}", component.len()))
            .collect::<Vec<_>>()
            .join("/");
        let qualified = format!("{encoded_scope}/{}:{role}/{}:{key}", role.len(), key.len(),);
        let local = role_local_id(&qualified, 0);
        if !self.allocated.insert(local) {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "persistent semantic name is duplicated or collided",
            ));
        }
        Ok(PersistentId::new(self.namespace, local))
    }

    pub fn push_scope(&mut self, role: impl Into<String>) -> KernelResult<()> {
        let role = role.into();
        if role.is_empty() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "persistent naming scope components must be nonempty",
            ));
        }
        self.scope.push(role);
        Ok(())
    }

    pub fn pop_scope(&mut self) -> KernelResult<()> {
        self.scope.pop().map(|_| ()).ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "persistent naming scope stack is empty",
            )
        })
    }
}

fn role_local_id(role: &str, ordinal: u64) -> u64 {
    // Stable FNV-1a rather than DefaultHasher, whose algorithm is not an API
    // guarantee. The namespace remains the operation identity; this hashes only
    // the semantic output role and its stable ordinal within that role.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in role.as_bytes().iter().copied().chain(ordinal.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
