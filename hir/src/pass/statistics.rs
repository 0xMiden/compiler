use alloc::{boxed::Box, format, vec::Vec};
use core::{any::Any, fmt};

use compact_str::CompactString;

use crate::Report;

/// A [Statistic] represents some stateful datapoint collected by and across passes.
///
/// Statistics are named, have a description, and have a value. The value can be pretty printed,
/// and multiple instances of the same statistic can be merged together.
#[derive(Clone)]
pub struct PassStatistic<V> {
    pub name: CompactString,
    pub description: CompactString,
    pub value: V,
}
impl<V> PassStatistic<V>
where
    V: StatisticValue,
{
    pub fn new(name: CompactString, description: CompactString, value: V) -> Self {
        Self {
            name,
            description,
            value,
        }
    }
}
impl<V> Eq for PassStatistic<V> {}
impl<V> PartialEq for PassStatistic<V> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl<V> PartialOrd for PassStatistic<V> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.name.cmp(&other.name))
    }
}
impl<V> Ord for PassStatistic<V> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}
impl<V> Statistic for PassStatistic<V>
where
    V: Clone + StatisticValue + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        self.value.pretty_print()
    }

    fn try_merge(&mut self, other: &mut dyn Any) -> Result<(), Report> {
        let lhs = &mut self.value;
        if let Some(rhs) = other.downcast_mut::<<V as StatisticValue>::Value>() {
            lhs.merge(rhs);
            Ok(())
        } else {
            let name = &self.name;
            let expected_ty = core::any::type_name::<<V as StatisticValue>::Value>();
            Err(Report::msg(format!(
                "could not merge statistic '{name}': expected value of type '{expected_ty}', but \
                 got a value of some other type"
            )))
        }
    }

    fn clone(&self) -> Box<dyn Statistic> {
        use core::clone::CloneToUninit;
        let mut this = Box::<Self>::new_uninit();
        unsafe {
            self.clone_to_uninit(this.as_mut_ptr().cast());
            this.assume_init()
        }
    }
}

/// An abstraction over statistics that allows operating generically over statistics with different
/// types of values.
pub trait Statistic {
    /// The display name of this statistic
    fn name(&self) -> &str;
    /// A description of what this statistic means and why it is significant
    fn description(&self) -> &str;
    /// Pretty prints this statistic as a value
    fn pretty_print(&self) -> crate::formatter::Document;
    /// Merges another instance of this statistic into this one, given a mutable reference to the
    /// raw underlying value of the other instance.
    ///
    /// Returns `Err` if `other` is not a valid value type for this statistic
    fn try_merge(&mut self, other: &mut dyn Any) -> Result<(), Report>;
    /// Clones the underlying statistic
    fn clone(&self) -> Box<dyn Statistic>;
}

pub trait StatisticValue {
    type Value: Any + Clone;

    fn value(&self) -> &Self::Value;
    fn value_mut(&mut self) -> &mut Self::Value;
    fn value_as_any(&self) -> &dyn Any {
        self.value() as &dyn Any
    }
    fn value_as_any_mut(&mut self) -> &mut dyn Any {
        self.value_mut() as &mut dyn Any
    }
    fn expected_type(&self) -> &'static str {
        core::any::type_name::<<Self as StatisticValue>::Value>()
    }
    fn merge(&mut self, other: &mut Self::Value);
    fn pretty_print(&self) -> crate::formatter::Document;
}

impl<V: Any + Clone> dyn StatisticValue<Value = V> {
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.value_as_any().downcast_ref::<T>()
    }

    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.value_as_any_mut().downcast_mut::<T>()
    }
}

/// Merges via OR
impl StatisticValue for bool {
    type Value = bool;

    fn value(&self) -> &Self::Value {
        self
    }

    fn value_mut(&mut self) -> &mut Self::Value {
        self
    }

    fn merge(&mut self, other: &mut Self::Value) {
        *self |= *other;
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        crate::formatter::display(*self)
    }
}

/// A boolean flag which evalutates to true, only if all observed values are false.
///
/// Defaults to false, and merges by OR.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct FlagNone(bool);
impl From<FlagNone> for bool {
    #[inline(always)]
    fn from(flag: FlagNone) -> Self {
        !flag.0
    }
}
impl StatisticValue for FlagNone {
    type Value = FlagNone;

    fn value(&self) -> &Self::Value {
        self
    }

    fn value_mut(&mut self) -> &mut Self::Value {
        self
    }

    fn merge(&mut self, other: &mut Self::Value) {
        if !self.0 && !other.0 {
            self.0 = true;
        } else {
            self.0 ^= other.0
        }
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        crate::formatter::display(bool::from(*self))
    }
}

/// A boolean flag which evaluates to true, only if at least one true value was observed.
///
/// Defaults to false, and merges by OR.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FlagAny(bool);
impl From<FlagAny> for bool {
    #[inline(always)]
    fn from(flag: FlagAny) -> Self {
        flag.0
    }
}
impl StatisticValue for FlagAny {
    type Value = FlagAny;

    fn value(&self) -> &Self::Value {
        self
    }

    fn value_mut(&mut self) -> &mut Self::Value {
        self
    }

    fn merge(&mut self, other: &mut Self::Value) {
        self.0 |= other.0;
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        crate::formatter::display(bool::from(*self))
    }
}

/// A boolean flag which evaluates to true, only if all observed values were true.
///
/// Defaults to true, and merges by AND.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FlagAll(bool);
impl From<FlagAll> for bool {
    #[inline(always)]
    fn from(flag: FlagAll) -> Self {
        flag.0
    }
}
impl StatisticValue for FlagAll {
    type Value = FlagAll;

    fn value(&self) -> &Self::Value {
        self
    }

    fn value_mut(&mut self) -> &mut Self::Value {
        self
    }

    fn merge(&mut self, other: &mut Self::Value) {
        self.0 &= other.0
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        crate::formatter::display(bool::from(*self))
    }
}

macro_rules! numeric_statistic {
    (#[cfg $($args:tt)*] $int_ty:ty) => {
        /// Adds two numbers by saturating addition
        #[cfg $($args)*]
        impl StatisticValue for $int_ty {
            type Value = $int_ty;
            fn value(&self) -> &Self::Value { self }
            fn value_mut(&mut self) -> &mut Self::Value { self }
            fn merge(&mut self, other: &mut Self::Value) {
                *self = self.saturating_add(*other);
            }
            fn pretty_print(&self) -> crate::formatter::Document {
                crate::formatter::display(*self)
            }
        }
    };

    (#[cfg $($args:tt)*] $int_ty:ty as $wrapper_ty:ty) => {
        /// Adds two numbers by saturating addition
        #[cfg $($args)*]
        impl StatisticValue for $int_ty {
            type Value = $int_ty;
            fn value(&self) -> &Self::Value { self }
            fn value_mut(&mut self) -> &mut Self::Value { self }
            fn merge(&mut self, other: &mut Self::Value) {
                *self = self.saturating_add(*other);
            }
            fn pretty_print(&self) -> crate::formatter::Document {
                crate::formatter::display(<$wrapper_ty>::from(*self))
            }
        }
    };

    ($int_ty:ty) => {
        /// Adds two numbers by saturating addition
        impl StatisticValue for $int_ty {
            type Value = $int_ty;
            fn value(&self) -> &Self::Value { self }
            fn value_mut(&mut self) -> &mut Self::Value { self }
            fn merge(&mut self, other: &mut Self::Value) {
                *self = self.saturating_add(*other);
            }
            fn pretty_print(&self) -> crate::formatter::Document {
                crate::formatter::display(*self)
            }
        }
    }
}

numeric_statistic!(u8);
numeric_statistic!(i8);
numeric_statistic!(u16);
numeric_statistic!(i16);
numeric_statistic!(u32);
numeric_statistic!(i32);
numeric_statistic!(u64);
numeric_statistic!(i64);
numeric_statistic!(usize);
numeric_statistic!(isize);
numeric_statistic!(
    #[cfg(feature = "std")]
    std::time::Duration as midenc_session::HumanDuration
);
numeric_statistic!(
    #[cfg(feature = "std")]
    midenc_session::HumanDuration
);

impl StatisticValue for f64 {
    type Value = f64;

    fn value(&self) -> &Self::Value {
        self
    }

    fn value_mut(&mut self) -> &mut Self::Value {
        self
    }

    fn merge(&mut self, other: &mut Self::Value) {
        *self += *other;
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        crate::formatter::display(*self)
    }
}

/// Merges an array of statistic values element-wise
impl<T, const N: usize> StatisticValue for [T; N]
where
    T: Any + StatisticValue + Clone,
{
    type Value = [T; N];

    fn value(&self) -> &Self::Value {
        self
    }

    fn value_mut(&mut self) -> &mut Self::Value {
        self
    }

    fn merge(&mut self, other: &mut Self::Value) {
        for index in 0..N {
            self[index].merge(other[index].value_mut());
        }
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        use crate::formatter::const_text;

        let doc = const_text("[");
        self.iter().enumerate().fold(doc, |mut doc, (i, item)| {
            if i > 0 {
                doc += const_text(", ");
            }
            doc + item.pretty_print()
        }) + const_text("]")
    }
}

/// Merges two vectors of statistics by appending
impl<T> StatisticValue for Vec<T>
where
    T: Any + StatisticValue + Clone,
{
    type Value = Vec<T>;

    fn value(&self) -> &Self::Value {
        self
    }

    fn value_mut(&mut self) -> &mut Self::Value {
        self
    }

    fn merge(&mut self, other: &mut Self::Value) {
        self.append(other);
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        use crate::formatter::const_text;

        let doc = const_text("[");
        self.iter().enumerate().fold(doc, |mut doc, (i, item)| {
            if i > 0 {
                doc += const_text(", ");
            }
            doc + item.pretty_print()
        }) + const_text("]")
    }
}

/// Merges two maps of statistics by merging values of identical keys, and appending missing keys
impl<K, V> StatisticValue for alloc::collections::BTreeMap<K, V>
where
    K: Ord + Clone + fmt::Display + 'static,
    V: Any + StatisticValue + Clone,
{
    type Value = alloc::collections::BTreeMap<K, V>;

    fn value(&self) -> &Self::Value {
        self
    }

    fn value_mut(&mut self) -> &mut Self::Value {
        self
    }

    fn merge(&mut self, other: &mut Self::Value) {
        use alloc::collections::btree_map::Entry;

        while let Some((k, mut v)) = other.pop_first() {
            match self.entry(k) {
                Entry::Vacant(entry) => {
                    entry.insert(v);
                }
                Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(v.value_mut());
                }
            }
        }
    }

    fn pretty_print(&self) -> crate::formatter::Document {
        use crate::formatter::{const_text, indent, nl, text, Document};
        if self.is_empty() {
            const_text("{}")
        } else {
            let single_line = const_text("{")
                + self.iter().enumerate().fold(Document::Empty, |mut doc, (i, (k, v))| {
                    if i > 0 {
                        doc += const_text(", ");
                    }
                    doc + text(format!("{k}: ")) + v.pretty_print()
                })
                + const_text("}");
            let multi_line = const_text("{")
                + indent(
                    4,
                    self.iter().enumerate().fold(nl(), |mut doc, (i, (k, v))| {
                        if i > 0 {
                            doc += const_text(",") + nl();
                        }
                        doc + text(format!("{k}: ")) + v.pretty_print()
                    }) + nl(),
                )
                + const_text("}");
            single_line | multi_line
        }
    }
}
