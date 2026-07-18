//! [`Pool<T>`] is a dense slot arena with ABA-safe generational handles.
//!
//! Parallel vectors store values, occupancy, generations, and a LIFO free list.
//! Values remain contiguous so [`Pool::visit_runs`] can borrow live `&[T]`
//! slices. A free slot holds an inexpensive [`SlotFill`] value until reuse or
//! compaction.
//!
//! [`compact`]: Pool::compact

use crate::container::CompactError;

/// Disposable value stored in a released slot of a dense [`Pool`].
///
/// The value is never observable and should be inexpensive to construct. This
/// trait does not imply a meaningful [`Default`] value.
pub(crate) trait SlotFill {
    /// Returns an inexpensive disposable value for a released slot.
    fn slot_fill() -> Self;
}

/// Physical pool-cell reference containing a slot index and generation.
///
/// A handle is not persistent entity identity and is never serialized. Its
/// derived ordering is useful for tests and auxiliary structures, not drawing
/// order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Handle {
    /// Slot index within the pool.
    pub(crate) index: u32,
    /// Generation issued with this handle; it must match the current slot.
    pub(crate) generation: u32,
}

/// Slot arena with generational reuse.
///
/// Removing a value advances its slot generation, so stale handles never resolve
/// after reuse. Cloning preserves the complete storage layout and its handles.
#[derive(Debug, Clone)]
pub(crate) struct Pool<T> {
    /// Dense value storage with one value per occupied or free slot.
    items: Vec<T>,
    /// Per-slot occupancy parallel to `items`.
    occupied: Vec<bool>,
    /// Current generation of each slot, parallel to `items`.
    gens: Vec<u32>,
    /// LIFO free list of reusable slot indices.
    free: Vec<u32>,
    /// Starting generation for newly appended slots.
    ///
    /// Compaction raises this above every issued generation so later appends
    /// cannot revive a handle that referred to a truncated slot.
    gen_floor: u32,
    /// Number of occupied cells.
    live: usize,
}

impl<T> Pool<T> {
    /// Returns a generation strictly greater than every generation issued here.
    pub(crate) fn next_compact_generation(&self) -> Result<u32, CompactError> {
        self.gens
            .iter()
            .copied()
            .fold(self.gen_floor, u32::max)
            .checked_add(1)
            .ok_or(CompactError::GenerationExhausted)
    }

    /// Creates an empty pool.
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            occupied: Vec::new(),
            gens: Vec::new(),
            free: Vec::new(),
            gen_floor: 0,
            live: 0,
        }
    }

    /// Inserts `value` and returns its [`Handle`].
    ///
    /// Reuses a free slot with its advanced generation or appends a new slot at
    /// `gen_floor`.
    pub(crate) fn insert(&mut self, value: T) -> Handle {
        self.live += 1;
        if let Some(index) = self.free.pop() {
            let i = index as usize;
            debug_assert!(!self.occupied[i], "free-list apuntaba a slot ocupado");
            self.items[i] = value; // Replace the disposable fill value.
            self.occupied[i] = true;
            Handle {
                index,
                generation: self.gens[i],
            }
        } else {
            let index = u32::try_from(self.items.len())
                .expect("recuento de slots del pool excede u32::MAX");
            self.items.push(value);
            self.occupied.push(true);
            self.gens.push(self.gen_floor);
            Handle {
                index,
                generation: self.gen_floor,
            }
        }
    }

    /// Returns the immutable cell for `handle`, or `None` when it does not resolve.
    pub(crate) fn get(&self, handle: Handle) -> Option<&T> {
        let i = handle.index as usize;
        if self.gens.get(i).copied() == Some(handle.generation) && self.occupied[i] {
            Some(&self.items[i])
        } else {
            None
        }
    }

    /// Returns the mutable cell for `handle`, or `None` when it does not resolve.
    pub(crate) fn get_mut(&mut self, handle: Handle) -> Option<&mut T> {
        let i = handle.index as usize;
        if self.gens.get(i).copied() == Some(handle.generation) && self.occupied[i] {
            Some(&mut self.items[i])
        } else {
            None
        }
    }

    /// Returns whether `handle` resolves to an occupied cell.
    pub(crate) fn contains(&self, handle: Handle) -> bool {
        let i = handle.index as usize;
        self.gens.get(i).copied() == Some(handle.generation) && self.occupied[i]
    }

    /// Returns the number of occupied cells.
    pub(crate) fn len(&self) -> usize {
        self.live
    }

    /// Returns whether the pool has no occupied cells.
    pub(crate) fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Iterates over handles for all occupied cells in physical slot order.
    pub(crate) fn iter_handles(&self) -> impl Iterator<Item = Handle> + '_ {
        self.occupied
            .iter()
            .enumerate()
            .filter(|&(_, &occ)| occ)
            .map(move |(i, _)| Handle {
                index: i as u32,
                generation: self.gens[i],
            })
    }

    /// Visits each maximal run of occupied physical slots as a contiguous slice.
    ///
    /// The callback also receives the first slot index so callers can align
    /// parallel columns. Holes split runs, and the total visited length equals
    /// [`len`](Self::len). Each slice is valid only during its callback.
    pub(crate) fn visit_runs<F>(&self, mut f: F)
    where
        F: FnMut(usize, &[T]),
    {
        let n = self.occupied.len();
        let mut i = 0;
        while i < n {
            if self.occupied[i] {
                let start = i;
                while i < n && self.occupied[i] {
                    i += 1;
                }
                f(start, &self.items[start..i]);
            } else {
                i += 1;
            }
        }
    }
}

impl<T: SlotFill> Pool<T> {
    /// Removes and returns the cell for `handle`, or `None` if it does not resolve.
    ///
    /// Removal leaves a fill value, advances the generation, and returns the slot
    /// to the free list. A slot already at `u32::MAX` is retired instead.
    pub(crate) fn remove(&mut self, handle: Handle) -> Option<T> {
        let i = handle.index as usize;
        if self.gens.get(i).copied() != Some(handle.generation) {
            return None;
        }
        if !self.occupied[i] {
            return None;
        }
        let value = std::mem::replace(&mut self.items[i], T::slot_fill());
        self.occupied[i] = false;
        self.live -= 1;
        if self.gens[i] == u32::MAX {
            // Retire the slot rather than reissuing its maximum generation.
        } else {
            self.gens[i] += 1;
            self.free.push(handle.index);
        }
        Some(value)
    }
}

impl<T> Pool<T> {
    /// Compacts live cells to the front and returns `(old_handle, new_handle)`
    /// pairs in their new physical order.
    ///
    /// Compaction leaves one contiguous live run and an empty free list. Every
    /// survivor receives `max_gen + 1`, invalidating all older handles. The method
    /// fails without moving data if `max_gen == u32::MAX`.
    pub(crate) fn compact(&mut self) -> Result<Vec<(Handle, Handle)>, CompactError> {
        let new_gen = self.next_compact_generation()?;
        let old_items = std::mem::take(&mut self.items);
        let old_occupied = std::mem::take(&mut self.occupied);
        let old_gens = std::mem::take(&mut self.gens);

        let mut remap = Vec::with_capacity(self.live);
        let mut items = Vec::with_capacity(self.live);
        let mut gens = Vec::with_capacity(self.live);

        for (old_index, ((value, occ), old_gen)) in old_items
            .into_iter()
            .zip(old_occupied)
            .zip(old_gens)
            .enumerate()
        {
            if occ {
                let new_index = items.len() as u32;
                remap.push((
                    Handle {
                        index: old_index as u32,
                        generation: old_gen,
                    },
                    Handle {
                        index: new_index,
                        generation: new_gen,
                    },
                ));
                items.push(value);
                gens.push(new_gen);
            }
            // Dropping the old vector releases fill values from holes.
        }

        self.occupied = vec![true; items.len()];
        self.items = items;
        self.gens = gens;
        self.free.clear();
        // Future appends start above every previously issued generation.
        self.gen_floor = new_gen;
        // Compaction preserves the number of live cells.
        Ok(remap)
    }

    #[cfg(test)]
    pub(crate) fn force_generation_exhaustion(&mut self) {
        if let Some(generation) = self.gens.first_mut() {
            *generation = u32::MAX;
        } else {
            self.gen_floor = u32::MAX;
        }
    }
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl SlotFill for u64 {
    fn slot_fill() -> Self {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use proptest::collection::vec;
    use proptest::prelude::*;

    #[test]
    fn insert_then_get_returns_value() {
        let mut p = Pool::<u64>::new();
        let h = p.insert(42);
        assert_eq!(p.get(h), Some(&42));
        assert!(p.contains(h));
        assert_eq!(p.len(), 1);
        assert!(!p.is_empty());
    }

    #[test]
    fn get_mut_mutates_in_place() {
        let mut p = Pool::<u64>::new();
        let h = p.insert(1);
        *p.get_mut(h).unwrap() = 99;
        assert_eq!(p.get(h), Some(&99));
    }

    #[test]
    fn removed_handle_never_resolves() {
        let mut p = Pool::<u64>::new();
        let h = p.insert(7);
        assert_eq!(p.remove(h), Some(7));
        // Removing twice is a no-op because the handle is stale.
        assert_eq!(p.remove(h), None);
        assert_eq!(p.get(h), None);
        assert!(!p.contains(h));
        assert!(p.is_empty());
    }

    #[test]
    fn recycled_slot_bumps_generation() {
        let mut p = Pool::<u64>::new();
        let h1 = p.insert(10);
        assert_eq!(p.remove(h1), Some(10));
        let h2 = p.insert(20);
        // The physical slot is reused...
        assert_eq!(h1.index, h2.index, "el slot libre debe reutilizarse");
        // ...with a new generation, so only the new handle resolves.
        assert_ne!(h1.generation, h2.generation);
        assert_eq!(p.get(h1), None);
        assert_eq!(p.get(h2), Some(&20));
    }

    #[test]
    fn stale_generation_does_not_resolve_new_occupant() {
        // A stale generation cannot access the new occupant of the same slot.
        let mut p = Pool::<u64>::new();
        let old = p.insert(100);
        p.remove(old);
        let _new = p.insert(200);
        assert_eq!(
            p.get(old),
            None,
            "gen antigua jamás resuelve al nuevo dueño"
        );
    }

    #[test]
    fn slot_retires_on_generation_overflow() {
        // Tests can set private fields to reach the overflow boundary directly.
        let mut p = Pool::<u64>::new();
        let _h0 = p.insert(10);
        p.gens[0] = u32::MAX;
        let h_max = Handle {
            index: 0,
            generation: u32::MAX,
        };
        assert_eq!(p.get(h_max), Some(&10));
        assert_eq!(p.remove(h_max), Some(10));
        // The retired slot is absent from the free list, so insertion appends.
        assert!(p.free.is_empty(), "el slot desbordado no debe reciclarse");
        let h2 = p.insert(20);
        assert_ne!(h2.index, 0, "el slot retirado no debe reutilizarse");
        assert_eq!(
            p.get(h_max),
            None,
            "el handle retirado está muerto para siempre"
        );
    }

    // ---------- visit_runs: maximal live runs ----------

    /// Collects `visit_runs` output as `(start, values)` pairs.
    fn runs_of(p: &Pool<u64>) -> Vec<(usize, Vec<u64>)> {
        let mut runs = Vec::new();
        p.visit_runs(|start, geos| runs.push((start, geos.to_vec())));
        runs
    }

    #[test]
    fn visit_runs_parte_en_los_huecos_y_suma_len() {
        let mut p = Pool::<u64>::new();
        let hs: Vec<Handle> = (0..6).map(|i| p.insert(i * 10)).collect();
        // Leave holes at indices 1 and 3; 0, 2, 4, and 5 remain live.
        p.remove(hs[1]);
        p.remove(hs[3]);

        let runs = runs_of(&p);
        // Maximal runs are [0], [2], and [4, 5].
        assert_eq!(runs, vec![(0, vec![0]), (2, vec![20]), (4, vec![40, 50]),]);
        // Run lengths sum to the live-cell count.
        let total: usize = runs.iter().map(|(_, v)| v.len()).sum();
        assert_eq!(total, p.len());
    }

    #[test]
    fn visit_runs_vacio_no_invoca() {
        let p = Pool::<u64>::new();
        assert!(runs_of(&p).is_empty());
    }

    // ---------- compact: remove holes and invalidate old handles ----------

    #[test]
    fn compact_deja_un_solo_tramo_y_conserva_valores() {
        let mut p = Pool::<u64>::new();
        let hs: Vec<Handle> = (0..6).map(|i| p.insert(i * 10)).collect();
        // Removing 1 and 3 leaves 0, 20, 40, and 50 in physical order.
        p.remove(hs[1]);
        p.remove(hs[3]);
        let before = p.len();

        let remap = p.compact().unwrap();

        // The live-cell count is preserved.
        assert_eq!(p.len(), before);
        // Survivors form one contiguous `0..len` run in order.
        let runs = runs_of(&p);
        assert_eq!(runs.len(), 1, "tras compact debe haber un solo tramo");
        assert_eq!(runs[0], (0, vec![0, 20, 40, 50]));

        // New handles resolve to the correct values...
        for (_old, new) in &remap {
            assert!(p.get(*new).is_some());
        }
        // ...while old handles no longer resolve.
        for (old, _new) in &remap {
            assert_eq!(
                p.get(*old),
                None,
                "handle viejo no debe resolver tras compact"
            );
        }

        // The remap covers exactly four live values and preserves order.
        let new_values: Vec<u64> = remap.iter().map(|(_, n)| *p.get(*n).unwrap()).collect();
        assert_eq!(new_values, vec![0, 20, 40, 50]);
    }

    #[test]
    fn compact_permite_insertar_sin_colisionar_con_handles_viejos() {
        let mut p = Pool::<u64>::new();
        let a = p.insert(1);
        let b = p.insert(2);
        p.remove(a);
        let remap = p.compact().unwrap(); // Only `b` survives and moves to index 0.
        assert_eq!(remap.len(), 1);
        let (_old_b, new_b) = remap[0];
        // Neither old handle resolves after compaction; the remapped handle does.
        assert!(p.get(a).is_none());
        assert!(p.get(b).is_none());
        assert_eq!(p.get(new_b), Some(&2));
        // Reusing the truncated index cannot revive `b`'s old handle.
        let c = p.insert(3);
        assert!(
            p.get(b).is_none(),
            "el handle viejo no debe revivir tras un insert"
        );
        assert!(p.get(a).is_none());
        assert_eq!(p.get(new_b), Some(&2));
        assert_eq!(p.get(c), Some(&3));
        assert_ne!(
            c, b,
            "el nuevo handle no debe igualar a uno pre-compactación"
        );
    }

    #[test]
    fn compact_rejects_generation_exhaustion_without_mutation() {
        let mut p = Pool::<u64>::new();
        let stale = p.insert(1);
        assert_eq!(p.remove(stale), Some(1));
        let live = p.insert(2);
        p.gens[live.index as usize] = u32::MAX;
        let live_max = Handle {
            index: live.index,
            generation: u32::MAX,
        };
        let before_items = p.items.clone();
        let before_occupied = p.occupied.clone();
        let before_gens = p.gens.clone();
        let before_free = p.free.clone();
        let before_floor = p.gen_floor;
        let before_live = p.live;

        assert_eq!(p.compact(), Err(CompactError::GenerationExhausted));
        assert_eq!(p.items, before_items);
        assert_eq!(p.occupied, before_occupied);
        assert_eq!(p.gens, before_gens);
        assert_eq!(p.free, before_free);
        assert_eq!(p.gen_floor, before_floor);
        assert_eq!(p.live, before_live);
        assert_eq!(p.get(live_max), Some(&2));
        assert_eq!(p.get(stale), None);

        let mut floor_exhausted = Pool::<u64>::new();
        let handle = floor_exhausted.insert(7);
        floor_exhausted.gen_floor = u32::MAX;
        assert_eq!(
            floor_exhausted.compact(),
            Err(CompactError::GenerationExhausted)
        );
        assert_eq!(floor_exhausted.get(handle), Some(&7));
    }

    // ---------- Property tests: random insert/remove/get sequences ----------

    #[derive(Debug, Clone)]
    enum Op {
        Insert(u64),
        Remove(usize),
        Get(usize),
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            any::<u64>().prop_map(Op::Insert),
            any::<usize>().prop_map(Op::Remove),
            any::<usize>().prop_map(Op::Get),
        ]
    }

    proptest! {
        #[test]
        fn random_ops_uphold_generation_contract(ops in vec(op_strategy(), 0..200)) {
            let mut pool = Pool::<u64>::new();
            // Reference model: live handles with values and released handles.
            let mut live: Vec<(Handle, u64)> = Vec::new();
            let mut freed: Vec<Handle> = Vec::new();

            for op in ops {
                match op {
                    Op::Insert(v) => {
                        let h = pool.insert(v);
                        // (b) A newly issued handle never collides with a released one.
                        prop_assert!(!freed.contains(&h), "handle reusado colisionó con uno liberado");
                        live.push((h, v));
                    }
                    Op::Remove(i) => {
                        if live.is_empty() {
                            continue;
                        }
                        let idx = i % live.len();
                        let (h, v) = live.remove(idx);
                        prop_assert_eq!(pool.remove(h), Some(v)); // (d) Returns the inserted value.
                        prop_assert_eq!(pool.remove(h), None); // Removing twice is a no-op.
                        freed.push(h);
                    }
                    Op::Get(i) => {
                        if live.is_empty() {
                            continue;
                        }
                        let idx = i % live.len();
                        let (h, v) = live[idx];
                        prop_assert_eq!(pool.get(h), Some(&v));
                    }
                }

                // (c) Length matches the reference model.
                prop_assert_eq!(pool.len(), live.len());
                prop_assert_eq!(pool.is_empty(), live.is_empty());

                // (a) Every released handle remains stale.
                for h in &freed {
                    prop_assert_eq!(pool.get(*h), None);
                    prop_assert!(!pool.contains(*h));
                }

                // (b)/(d) Every live handle resolves to its exact value.
                for (h, v) in &live {
                    prop_assert_eq!(pool.get(*h), Some(v));
                    prop_assert!(pool.contains(*h));
                }

                // `iter_handles` yields exactly the live set.
                let mut got: Vec<Handle> = pool.iter_handles().collect();
                got.sort_unstable();
                let mut want: Vec<Handle> = live.iter().map(|(h, _)| *h).collect();
                want.sort_unstable();
                prop_assert_eq!(got, want);

                // `visit_runs` covers every live value and its run lengths sum to `len`.
                let mut run_total = 0usize;
                let mut run_values: Vec<u64> = Vec::new();
                pool.visit_runs(|_start, geos| {
                    run_total += geos.len();
                    run_values.extend_from_slice(geos);
                });
                prop_assert_eq!(run_total, live.len());
                run_values.sort_unstable();
                let mut want_values: Vec<u64> = live.iter().map(|(_, v)| *v).collect();
                want_values.sort_unstable();
                prop_assert_eq!(run_values, want_values);
            }
        }

        #[test]
        fn compact_preserva_vivas_y_mata_handles_viejos(ops in vec(op_strategy(), 0..120)) {
            use std::collections::HashMap;

            let mut pool = Pool::<u64>::new();
            let mut live: Vec<(Handle, u64)> = Vec::new();
            let mut freed: Vec<Handle> = Vec::new();

            for op in ops {
                match op {
                    Op::Insert(v) => live.push((pool.insert(v), v)),
                    Op::Remove(i) => {
                        if !live.is_empty() {
                            let (h, _) = live.remove(i % live.len());
                            pool.remove(h);
                            freed.push(h);
                        }
                    }
                    Op::Get(_) => {}
                }
            }

            let pre_live = live.clone();
            let expected = pre_live.len();
            let val_of: HashMap<Handle, u64> = pre_live.iter().copied().collect();

            let remap = pool.compact().unwrap();

            // The remap covers every survivor; new handles resolve and old ones do not.
            prop_assert_eq!(remap.len(), expected);
            for (old, new) in &remap {
                prop_assert!(val_of.contains_key(old), "remap incluye un handle no-vivo");
                prop_assert_eq!(pool.get(*new), Some(&val_of[old]));
                prop_assert_eq!(pool.get(*old), None);
            }
            // Handles released before compaction remain stale.
            for h in &freed {
                prop_assert_eq!(pool.get(*h), None);
            }
            // Every pre-compaction handle, live or released, is stale.
            for (h, _) in &pre_live {
                prop_assert_eq!(pool.get(*h), None);
            }
            // One live run remains and `len` is preserved.
            let mut runs = 0usize;
            let mut total = 0usize;
            pool.visit_runs(|_start, geos| {
                runs += 1;
                total += geos.len();
            });
            prop_assert!(runs <= 1, "tras compact debe haber a lo sumo un tramo");
            prop_assert_eq!(total, expected);
            prop_assert_eq!(pool.len(), expected);

            // Later insertions cannot revive handles from before compaction.
            for k in 0..6u64 {
                pool.insert(9000 + k);
            }
            for (h, _) in &pre_live {
                prop_assert_eq!(pool.get(*h), None);
            }
            for h in &freed {
                prop_assert_eq!(pool.get(*h), None);
            }
        }
    }
}
