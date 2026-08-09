use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use super::{DiagnosticKind, KernelError, KernelResult};

/// A typed, generational reference into a kernel-owned arena.
pub struct Handle<T> {
    index: u32,
    generation: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    pub const fn index(self) -> u32 {
        self.index
    }
    pub const fn generation(self) -> u32 {
        self.generation
    }

    const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            marker: PhantomData,
        }
    }
}

impl<T> Copy for Handle<T> {}
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}
impl<T> Eq for Handle<T> {}
impl<T> PartialOrd for Handle<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Handle<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.index, self.generation).cmp(&(other.index, other.generation))
    }
}
impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}
impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.index, self.generation)
    }
}

#[derive(Debug, Clone)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Compact storage with stale-handle detection and deterministic iteration order.
#[derive(Debug, Clone)]
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn insert(&mut self, value: T) -> Handle<T> {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.value = Some(value);
            Handle::new(index, slot.generation)
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                value: Some(value),
            });
            Handle::new(index, 0)
        }
    }

    /// Insert a value that needs to contain its own handle.
    pub(crate) fn insert_with(&mut self, make: impl FnOnce(Handle<T>) -> T) -> Handle<T> {
        if let Some(index) = self.free.pop() {
            let generation = self.slots[index as usize].generation;
            let handle = Handle::new(index, generation);
            self.slots[index as usize].value = Some(make(handle));
            self.len += 1;
            handle
        } else {
            let index = self.slots.len() as u32;
            let handle = Handle::new(index, 0);
            self.slots.push(Slot {
                generation: 0,
                value: Some(make(handle)),
            });
            self.len += 1;
            handle
        }
    }

    pub fn get(&self, handle: Handle<T>) -> KernelResult<&T> {
        let slot = self.slots.get(handle.index as usize).ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::MissingEntity,
                format!("arena handle index {} does not exist", handle.index),
            )
        })?;
        if slot.generation != handle.generation || slot.value.is_none() {
            return Err(KernelError::new(
                DiagnosticKind::StaleHandle,
                format!("arena handle {:?} is stale", handle),
            ));
        }
        slot.value.as_ref().ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::StaleHandle,
                format!("arena handle {:?} has no live value", handle),
            )
        })
    }

    pub(crate) fn get_mut(&mut self, handle: Handle<T>) -> KernelResult<&mut T> {
        let slot = self.slots.get_mut(handle.index as usize).ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::MissingEntity,
                format!("arena handle index {} does not exist", handle.index),
            )
        })?;
        if slot.generation != handle.generation || slot.value.is_none() {
            return Err(KernelError::new(
                DiagnosticKind::StaleHandle,
                format!("arena handle {:?} is stale", handle),
            ));
        }
        slot.value.as_mut().ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::StaleHandle,
                format!("arena handle {:?} has no live value", handle),
            )
        })
    }

    pub(crate) fn remove(&mut self, handle: Handle<T>) -> KernelResult<T> {
        let slot = self.slots.get_mut(handle.index as usize).ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::MissingEntity,
                format!("arena handle index {} does not exist", handle.index),
            )
        })?;
        if slot.generation != handle.generation || slot.value.is_none() {
            return Err(KernelError::new(
                DiagnosticKind::StaleHandle,
                format!("arena handle {:?} is stale", handle),
            ));
        }
        let value = slot.value.take().ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::StaleHandle,
                format!("arena handle {:?} has no live value", handle),
            )
        })?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(handle.index);
        self.len -= 1;
        Ok(value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value
                .as_ref()
                .map(|value| (Handle::new(index as u32, slot.generation), value))
        })
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}
