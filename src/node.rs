use std::{
    ops::{Index, IndexMut},
    sync::atomic::{AtomicU64, Ordering},
};

/// A scene-scoped, generational identity. It is not a layout or GPU handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    owner: u64,
    slot: usize,
    generation: u64,
}

struct Slot<T> {
    generation: u64,
    value: Option<T>,
}

pub(crate) struct Arena<T> {
    owner: u64,
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
    live: usize,
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT_OWNER
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("scene identities exhausted");
        Self {
            owner,
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
        }
    }

    pub fn insert(&mut self, value: T) -> NodeId {
        let slot = self.free.pop().unwrap_or_else(|| {
            self.slots.push(Slot {
                generation: 0,
                value: None,
            });
            self.slots.len() - 1
        });
        self.slots[slot].value = Some(value);
        self.live += 1;
        NodeId {
            owner: self.owner,
            slot,
            generation: self.slots[slot].generation,
        }
    }

    pub fn owns(&self, id: NodeId) -> bool {
        self.owner == id.owner
    }

    pub fn get(&self, id: NodeId) -> Option<&T> {
        self.slots
            .get(id.slot)
            .filter(|slot| self.owns(id) && slot.generation == id.generation)?
            .value
            .as_ref()
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut T> {
        if !self.owns(id) {
            return None;
        }
        self.slots
            .get_mut(id.slot)
            .filter(|slot| slot.generation == id.generation)?
            .value
            .as_mut()
    }

    pub fn remove(&mut self, id: NodeId) -> Option<T> {
        self.get(id)?;
        let slot = &mut self.slots[id.slot];
        let value = slot.value.take();
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(id.slot);
        }
        self.live -= 1;
        value
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().filter_map(|slot| slot.value.as_ref())
    }
    pub fn len(&self) -> usize {
        self.live
    }
    pub fn slot_capacity(&self) -> usize {
        self.slots.len()
    }
}

impl<T> Index<NodeId> for Arena<T> {
    type Output = T;
    fn index(&self, id: NodeId) -> &T {
        self.get(id).expect("live internal node")
    }
}

impl<T> IndexMut<NodeId> for Arena<T> {
    fn index_mut(&mut self, id: NodeId) -> &mut T {
        self.get_mut(id).expect("live internal node")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_and_foreign_ids_never_alias_reused_slots() {
        let mut arena = Arena::new();
        let old = arena.insert(1);
        assert_eq!(arena.remove(old), Some(1));
        let new = arena.insert(2);
        assert_eq!(old.slot, new.slot);
        assert_ne!(old, new);
        assert_eq!(arena.get(old), None);
        assert_eq!(arena.get(new), Some(&2));
        assert_eq!(Arena::<i32>::new().get(new), None);
    }

    #[test]
    fn exhausted_generations_retire_instead_of_wrapping() {
        let mut arena = Arena::new();
        let original = arena.insert(1);
        arena.slots[original.slot].generation = u64::MAX;
        let last = NodeId {
            generation: u64::MAX,
            ..original
        };
        assert_eq!(arena.remove(last), Some(1));
        let fresh = arena.insert(2);
        assert_ne!(last.slot, fresh.slot);
        assert_eq!(arena.get(last), None);
    }
}
