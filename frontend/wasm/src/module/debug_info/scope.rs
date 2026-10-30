use alloc::{rc::Rc, vec::Vec};

use super::{LocationDescriptor, locations::normalize_locations};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeRange {
    pub start: u64,
    pub end: Option<u64>,
}

impl ScopeRange {
    fn intersection(self, other: Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = match (self.end, other.end) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        end.is_none_or(|end| start < end).then_some(Self { start, end })
    }
}

#[derive(Debug)]
pub(crate) struct VariableScope {
    pub ranges: Vec<ScopeRange>,
    pub depth: usize,
    parent: Option<Rc<Self>>,
}

impl VariableScope {
    pub fn new(mut ranges: Vec<ScopeRange>, parent: Option<Rc<Self>>) -> Rc<Self> {
        ranges.retain(|range| range.end.is_none_or(|end| range.start < end));
        ranges.sort_by_key(|range| range.start);
        let mut normalized: Vec<ScopeRange> = Vec::new();
        for range in ranges {
            if let Some(previous) = normalized.last_mut()
                && previous.end.is_none_or(|end| range.start <= end)
            {
                previous.end = previous.end.zip(range.end).map(|(left, right)| left.max(right));
            } else {
                normalized.push(range);
            }
        }
        if let Some(parent) = &parent {
            let mut intersections = Vec::new();
            let mut parent_index = 0;
            for range in normalized {
                while parent.ranges.get(parent_index).is_some_and(|parent_range| {
                    parent_range.end.is_some_and(|end| end <= range.start)
                }) {
                    parent_index += 1;
                }
                for parent_range in &parent.ranges[parent_index..] {
                    if range.end.is_some_and(|end| parent_range.start >= end) {
                        break;
                    }
                    if let Some(intersection) = range.intersection(*parent_range) {
                        intersections.push(intersection);
                    }
                }
            }
            normalized = intersections;
        }
        Rc::new(Self {
            ranges: normalized,
            depth: parent.as_ref().map_or(0, |parent| parent.depth + 1),
            parent,
        })
    }

    pub fn contains(&self, offset: u64) -> bool {
        self.ranges
            .get(self.ranges.partition_point(|range| range.start <= offset).wrapping_sub(1))
            .is_some_and(|range| range.end.is_none_or(|end| offset < end))
    }

    pub fn is_ancestor_of(&self, other: &Self) -> bool {
        let mut current = Some(other);
        while let Some(scope) = current {
            if core::ptr::eq(self, scope) {
                return true;
            }
            current = scope.parent.as_deref();
        }
        false
    }

    pub fn restrict_locations(
        &self,
        locations: Vec<LocationDescriptor>,
    ) -> Vec<LocationDescriptor> {
        let mut restricted = Vec::new();
        for location in locations {
            let range = ScopeRange {
                start: location.start,
                end: location.end,
            };
            let first = self
                .ranges
                .partition_point(|scope| scope.end.is_some_and(|end| end <= range.start));
            for scope in &self.ranges[first..] {
                if range.end.is_some_and(|end| scope.start >= end) {
                    break;
                }
                if let Some(range) = range.intersection(*scope) {
                    restricted.push(LocationDescriptor {
                        start: range.start,
                        end: range.end,
                        storage: location.storage.clone(),
                    });
                }
            }
        }
        normalize_locations(restricted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::debug_info::Expression;

    #[test]
    fn lexical_ranges_are_intersected_with_all_parent_fragments() {
        let parent = VariableScope::new(
            vec![
                ScopeRange {
                    start: 10,
                    end: Some(20),
                },
                ScopeRange {
                    start: 30,
                    end: Some(40),
                },
            ],
            None,
        );
        let child = VariableScope::new(
            vec![ScopeRange {
                start: 0,
                end: Some(50),
            }],
            Some(parent.clone()),
        );
        assert_eq!(child.ranges, parent.ranges);
        assert!(parent.is_ancestor_of(&child));
        assert!(!child.is_ancestor_of(&parent));
        for offset in [10, 19, 30, 39] {
            assert!(child.contains(offset));
        }
        for offset in [0, 9, 20, 29, 40, 50] {
            assert!(!child.contains(offset));
        }
        let locations = child.restrict_locations(vec![LocationDescriptor {
            start: 15,
            end: Some(35),
            storage: Expression::default(),
        }]);
        assert_eq!(
            locations
                .iter()
                .map(|location| (location.start, location.end))
                .collect::<Vec<_>>(),
            [(15, Some(20)), (30, Some(35))]
        );
    }
}
