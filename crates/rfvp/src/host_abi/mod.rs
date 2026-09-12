//! Versioned host ABI support.
//!
//! This module is intentionally isolated from the legacy `rfvp_*` exports.
//! The v1 table is a layout contract until the runtime and frame adapter are
//! wired through it.

use alloc::vec::Vec;
use core::marker::PhantomData;

pub mod v1;

#[no_mangle]
pub unsafe extern "C" fn rfvp_get_api_v1(out_size: *mut usize) -> *const v1::RfvpApiV1 {
    if !out_size.is_null() {
        unsafe { *out_size = core::mem::size_of::<v1::RfvpApiV1>() };
    }
    core::ptr::addr_of!(v1::API_V1)
}

#[cfg(all(
    not(feature = "no_std"),
    feature = "host-runtime",
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
pub mod runtime;

/// Typed, generation-safe handle used by host-side object registries.
#[repr(transparent)]
pub struct Handle<T> {
    raw: u64,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    pub const INVALID: Self = Self {
        raw: 0,
        marker: PhantomData,
    };

    pub const fn from_raw(raw: u64) -> Self {
        Self {
            raw,
            marker: PhantomData,
        }
    }

    pub const fn raw(self) -> u64 {
        self.raw
    }

    pub const fn is_valid(self) -> bool {
        self.raw != 0
    }

    fn from_parts(index: u32, generation: u32) -> Self {
        Self::from_raw(((generation as u64) << 32) | index as u64)
    }

    fn parts(self) -> Option<(u32, u32)> {
        if !self.is_valid() {
            return None;
        }
        Some((self.raw as u32, (self.raw >> 32) as u32))
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> Default for Handle<T> {
    fn default() -> Self {
        Self::INVALID
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Handle<T> {}

impl<T> core::hash::Hash for Handle<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<T> core::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Handle").field(&self.raw).finish()
    }
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Sparse registry backing the ABI's `uint64_t` object handles.
///
/// Handles encode a one-based generation and a vector slot. Removed slots are
/// reused only after their generation advances, so a stale handle cannot
/// address a newly inserted object.
pub struct HandleRegistry<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> HandleRegistry<T> {
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|slot| slot.value.is_none())
    }

    pub fn insert(&mut self, value: T) -> Handle<T> {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.value.is_none());
            slot.generation = next_generation(slot.generation);
            slot.value = Some(value);
            return Handle::from_parts(index, slot.generation);
        }

        let index = u32::try_from(self.slots.len()).expect("RFVP ABI handle registry exhausted");
        self.slots.push(Slot {
            generation: 1,
            value: Some(value),
        });
        Handle::from_parts(index, 1)
    }

    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        let (index, generation) = handle.parts()?;
        let slot = self.slots.get(index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        let (index, generation) = handle.parts()?;
        let slot = self.slots.get_mut(index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_mut())
            .flatten()
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let (index, generation) = handle.parts()?;
        let slot = self.slots.get_mut(index as usize)?;
        if slot.generation != generation {
            return None;
        }
        let value = slot.value.take()?;
        self.free.push(index);
        Some(value)
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.free.clear();
    }
}

const fn next_generation(current: u32) -> u32 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Runtime(u32);

    #[test]
    fn invalid_and_wrong_category_handles_do_not_resolve() {
        let mut runtimes = HandleRegistry::<Runtime>::new();
        let handle = runtimes.insert(Runtime(7));

        assert_eq!(runtimes.get(Handle::<Runtime>::INVALID), None);
        assert_eq!(runtimes.get(handle).map(|value| value.0), Some(7));
    }

    #[test]
    fn removed_slots_advance_generation_before_reuse() {
        let mut registry = HandleRegistry::<u32>::new();
        let first = registry.insert(10);
        assert_eq!(registry.remove(first), Some(10));
        assert_eq!(registry.get(first), None);

        let second = registry.insert(20);
        assert_ne!(first.raw(), second.raw());
        assert_eq!(registry.get(first), None);
        assert_eq!(registry.get(second), Some(&20));
    }

    #[test]
    fn clearing_invalidates_all_handles() {
        let mut registry = HandleRegistry::<u32>::new();
        let handle = registry.insert(1);
        registry.clear();
        assert_eq!(registry.get(handle), None);
        assert!(registry.is_empty());
    }
}
