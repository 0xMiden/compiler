use smallvec::SmallVec;

use crate::{
    dataflow::{ChangeResult, LatticeLike},
    ValueRef,
};

/// Represents a single value and its next use distance at some program point
#[derive(Debug, Copy, Clone)]
pub struct NextUse {
    /// The value in question
    pub value: ValueRef,
    /// The distance to its next use.
    ///
    /// The distance is `u32::MAX` if unused/unknown, 0 if used at the current program point
    pub distance: u32,
}

impl NextUse {
    #[inline]
    pub const fn is_live(&self) -> bool {
        self.distance < u32::MAX
    }
}

impl Eq for NextUse {}
impl PartialEq for NextUse {
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}
impl PartialOrd for NextUse {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for NextUse {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.distance
            .cmp(&other.distance)
            .then_with(|| self.value.borrow().id().cmp(&other.value.borrow().id()))
    }
}

/// The lattice representing global next-use information for a program point.
///
/// The lattice maps zero or more SSA values to their global next-use distance at a given program
/// point. It has set-like semantics: a value only appears once in each set, and the union of two
/// sets takes the minimum distance for values present in both sets.
///
/// From this, we derive the partial order required for a join semi-lattice:
///
/// * The _bottom_ value of the lattice, which represents uninitialized or unknown state, is the
///   empty set. A set which does not contain a specific value, indicates that we have not observed
///   any uses of that value at that point, so we cannot reason about it until we do, or until we
///   have reached its definition.
/// * The _top_ value of the lattice, which represents that a value is never used, is given by a
///   next-use distance of `u32::MAX`. This value is only applied when we reach the definition of
///   a value and have observed no uses of it.
/// * All other values represent the fact that a value has a given _minimal_ next-use distance. If
///   we observe two different next-use distances at some join point in the program, the larger
///   distance is discarded in favor of the shorter distance.
///
/// For known distances, there are a few distance-related properties that are worth keeping in mind:
///
/// * A distance of `0` indicates that the next-use of a value is at the current operation/anchor
/// * A distance of `1` indicates that the next-use of a value is at the operation immediately
///   succeeding the current operation.
/// * Distances are incremented by 1 at each program point preceding a use. The exception to this
///   is when a loop exit is reached, in which case all next-use distances across that edge are
///   incremented by a large constant value, 10,000, to ensure that next-use distances reflect the
///   fact that a use may actually be much further away than it appears, depending on how many
///   iterations of the loop occur before exiting the loop.
///
/// This lattice lets us answer the following questions:
///
/// 1. Is a given value live at the current program point
/// 2. Given the set of live values at the current program point, which values have the closest
///    (or furthest) next use?
///
/// The second question is of primary importance for spills analysis, register allocation and (in
/// the case of Miden) operand stack management. If we're going to choose what values to spill, so
/// as to keep the most important values available in registers (or the operand stack), then we
/// want to know when those values are needed.
#[derive(Default, Debug, Clone)]
pub struct NextUseSet(SmallVec<[NextUse; 4]>);

impl Eq for NextUseSet {}
impl PartialEq for NextUseSet {
    fn eq(&self, other: &Self) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }

        for next_use in self.0.iter() {
            if !other
                .0
                .iter()
                .find(|nu| nu.value == next_use.value)
                .is_some_and(|nu| nu.distance == next_use.distance)
            {
                return false;
            }
        }

        true
    }
}

impl LatticeLike for NextUseSet {
    #[inline]
    fn join(&self, other: &Self) -> Self {
        self.union(other)
    }

    #[inline]
    fn meet(&self, other: &Self) -> Self {
        self.intersection(other)
    }
}

impl FromIterator<NextUse> for NextUseSet {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = NextUse>,
    {
        let mut set = Self::default();
        for NextUse { value, distance } in iter.into_iter() {
            set.insert(value, distance);
        }
        set
    }
}

impl NextUseSet {
    /// Inserts `value` in this set with the given `distance`.
    ///
    /// A distance of `u32::MAX` signifies infinite distance, which is
    /// equivalent to saying that `value` is not live.
    ///
    /// If `value` is already in this set, the distance is updated to be the
    /// lesser of the two distances, e.g. if the previous distance was `u32::MAX`,
    /// and `distance` was `1`, the entry is updated to have a distance of `1` after
    /// this function returns.
    pub fn insert(&mut self, value: ValueRef, distance: u32) -> ChangeResult {
        if let Some(existing) = self.0.iter_mut().find(|next_use| next_use.value.eq(&value)) {
            if existing.distance == distance {
                ChangeResult::Unchanged
            } else {
                existing.distance = core::cmp::min(existing.distance, distance);
                ChangeResult::Changed
            }
        } else {
            self.0.push(NextUse { value, distance });
            ChangeResult::Changed
        }
    }

    /// Returns true if this set is empty/uninitialized
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of values recorded in this set
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the number of live values in this set, i.e. values whose next-use distance is not
    /// overdefined.
    pub fn num_live(&self) -> usize {
        self.0.iter().filter(|nu| nu.distance < u32::MAX).count()
    }

    /// Returns `true` if `value` is live in this set
    #[inline]
    pub fn is_live(&self, value: &ValueRef) -> bool {
        self.distance(value) < u32::MAX
    }

    /// Returns the distance to the next use of `value` as an integer.
    ///
    /// If `value` is not live, or the distance is unknown, returns `u32::MAX`
    pub fn distance(&self, value: &ValueRef) -> u32 {
        self.get(value).map(|next_use| next_use.distance).unwrap_or(u32::MAX)
    }

    /// Returns `true` if `value` is in this set
    #[inline]
    pub fn contains(&self, value: &ValueRef) -> bool {
        self.get(value).is_none()
    }

    /// Gets the [NextUse] associated with the given `value`, if known
    #[inline]
    pub fn get(&self, value: &ValueRef) -> Option<&NextUse> {
        self.0.iter().find(|next_use| &next_use.value == value)
    }

    /// Gets a mutable reference to the distance associated with the given `value`, if known
    #[inline]
    pub fn get_mut(&mut self, value: &ValueRef) -> Option<&mut NextUse> {
        self.0.iter_mut().find(|next_use| &next_use.value == value)
    }

    /// Removes the entry for `value` from this set
    pub fn remove(&mut self, value: &ValueRef) -> Option<u32> {
        self.0
            .iter()
            .position(|next_use| &next_use.value == value)
            .map(|index| self.0.swap_remove(index).distance)
    }

    /// Remove any entries for which `callback` returns `false`
    pub fn retain<F>(&mut self, callback: F)
    where
        F: FnMut(&mut NextUse) -> bool,
    {
        self.0.retain(callback);
    }

    /// Remove all entries in the set
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Returns a new set containing the union of `self` and `other`.
    ///
    /// The resulting set will preserve the minimum distances from both sets.
    pub fn union(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for NextUse { value, distance } in other.iter().cloned() {
            result.insert(value, distance);
        }
        result
    }

    /// Returns a new set containing the intersection of `self` and `other`.
    ///
    /// The resulting set will preserve the minimum distances from both sets.
    pub fn intersection(&self, other: &Self) -> Self {
        let mut result = Self::default();
        for NextUse {
            value,
            distance: v1,
        } in self.iter()
        {
            match other.get(value) {
                None => continue,
                Some(NextUse { distance: v2, .. }) => {
                    result.insert(*value, core::cmp::min(*v1, *v2));
                }
            }
        }
        result
    }

    /// Returns a new set containing the symmetric difference of `self` and `other`,
    /// i.e. the values that are in `self` or `other` but not in both.
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        let mut result = Self::default();
        for next_use in self.iter() {
            if !other.contains(&next_use.value) {
                result.0.push(*next_use);
            }
        }
        for next_use in other.iter() {
            if !self.contains(&next_use.value) {
                result.0.push(*next_use);
            }
        }
        result
    }

    pub fn iter(&self) -> impl Iterator<Item = &NextUse> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut NextUse> {
        self.0.iter_mut()
    }

    pub fn keys(&self) -> impl Iterator<Item = ValueRef> + '_ {
        self.0.iter().map(|next_use| next_use.value)
    }

    /// Returns an iterator over the values in this set with a finite next-use distance
    pub fn live(&self) -> impl Iterator<Item = ValueRef> + '_ {
        self.0.iter().filter_map(|next_use| {
            if next_use.distance < u32::MAX {
                Some(next_use.value)
            } else {
                None
            }
        })
    }

    /// Remove the value in this set which is closest compared to the others
    ///
    /// If this set is empty, returns `None`.
    ///
    /// If more than one value have the same distance, this returns the value with
    /// the lowest id first.
    #[inline]
    pub fn pop_first(&mut self) -> Option<NextUse> {
        let index =
            self.0.iter().enumerate().min_by(|a, b| a.1.cmp(b.1)).map(|(index, _)| index)?;
        Some(self.0.swap_remove(index))
    }

    /// Remove the value in this set which is furthest away compared to the others
    ///
    /// If this set is empty, returns `None`.
    ///
    /// If more than one value have the same distance, this returns the value with
    /// the highest id first.
    #[inline]
    pub fn pop_last(&mut self) -> Option<NextUse> {
        let index =
            self.0.iter().enumerate().max_by(|a, b| a.1.cmp(b.1)).map(|(index, _)| index)?;
        Some(self.0.swap_remove(index))
    }
}
impl<'a, 'b> core::ops::BitOr<&'b NextUseSet> for &'a NextUseSet {
    type Output = NextUseSet;

    #[inline]
    fn bitor(self, rhs: &'b NextUseSet) -> Self::Output {
        self.union(rhs)
    }
}
impl<'a, 'b> core::ops::BitAnd<&'b NextUseSet> for &'a NextUseSet {
    type Output = NextUseSet;

    #[inline]
    fn bitand(self, rhs: &'b NextUseSet) -> Self::Output {
        self.intersection(rhs)
    }
}
impl<'a, 'b> core::ops::BitXor<&'b NextUseSet> for &'a NextUseSet {
    type Output = NextUseSet;

    #[inline]
    fn bitxor(self, rhs: &'b NextUseSet) -> Self::Output {
        self.symmetric_difference(rhs)
    }
}
