#[cfg(feature = "std")]
pub use clap::ArgMatches;

#[cfg(not(feature = "std"))]
pub use self::fallback::ArgMatches;

mod fallback {
    #![allow(unused)]
    use alloc::{
        borrow::Cow,
        collections::{BTreeMap, VecDeque},
        format,
        sync::Arc,
        vec,
        vec::Vec,
    };
    use core::any::Any;

    use crate::{diagnostics::Report, CompileFlag, FlagAction};

    /// Violation of [`ArgMatches`] assumptions
    #[derive(Clone, Debug, thiserror::Error)]
    #[non_exhaustive]
    pub enum MatchesError {
        /// Failed to downcast `AnyValue` to the specified type
        #[non_exhaustive]
        #[error("could not downcast to {expected:?}, need to downcast to {actual:?}")]
        Downcast {
            /// Type for value stored in [`ArgMatches`][crate::ArgMatches]
            actual: AnyValueId,
            /// The target type to downcast to
            expected: AnyValueId,
        },
        /// Argument not defined in [`Command`][crate::Command]
        #[non_exhaustive]
        #[error(
            "unknown argument or group id.  Make sure you are using the argument id and not the \
             short or long flags"
        )]
        UnknownArgument {
            // Missing `id` but blocked on a public id type which will hopefully come with `unstable-v4`
        },
    }

    impl MatchesError {
        #[cfg_attr(debug_assertions, track_caller)]
        pub(crate) fn unwrap<T>(id: &str, r: Result<T, MatchesError>) -> T {
            let err = match r {
                Ok(t) => {
                    return t;
                }
                Err(err) => err,
            };
            panic!("Mismatch between definition and access of `{id}`. {err}",)
        }
    }

    #[derive(Default)]
    pub struct ArgMatches {
        #[cfg(debug_assertions)]
        valid_args: Vec<&'static str>,
        args: BTreeMap<&'static str, MatchedArg>,
    }

    impl ArgMatches {
        pub fn parse(
            mut argv: VecDeque<Cow<'static, str>>,
            flags: &BTreeMap<&'static str, &'static CompileFlag>,
        ) -> Result<Self, Report> {
            let mut this = Self::default();

            for flag in flags.values().copied() {
                this.register_flag(flag);
            }

            let mut trailing = false;
            while let Some(arg) = argv.pop_front() {
                if trailing {
                    this.args.get_mut("").unwrap().raw_vals.last_mut().unwrap().push(arg);
                    continue;
                }

                let flag = match arg.strip_prefix("--") {
                    Some("") => {
                        // Start a new trailing argument group
                        trailing = true;
                        this.args.insert(
                            "",
                            MatchedArg {
                                source: None,
                                indices: vec![],
                                type_id: None,
                                vals: vec![],
                                raw_vals: vec![],
                                ignore_case: false,
                            },
                        );
                        continue;
                    }
                    Some(name) => flags.get(name).copied(),
                    None => match arg.strip_prefix("-") {
                        Some("") => {
                            return Err(Report::msg(format!(
                                "unexpected positional argument: '{arg}'"
                            )));
                        }
                        Some(short) => {
                            let short = short.chars().next().unwrap();
                            flags
                                .values()
                                .copied()
                                .find(|flag| flag.short.is_some_and(|c| c == short))
                        }
                        None => {
                            return Err(Report::msg(format!(
                                "unexpected positional argument: '{arg}'"
                            )))
                        }
                    },
                };

                let flag = flag.ok_or_else(|| Report::msg(MatchesError::UnknownArgument {}))?;
                let flag_matches = this
                    .args
                    .get_mut(flag.name)
                    .ok_or_else(|| Report::msg(MatchesError::UnknownArgument {}))?;
                match flag.action {
                    FlagAction::Set => {
                        let value = argv
                            .pop_front()
                            .or(flag.default_missing_value.map(Cow::Borrowed))
                            .or(flag.default_value.map(Cow::Borrowed));
                        if let Some(value) = value {
                            flag_matches.raw_vals.push(vec![value]);
                        } else {
                            return Err(Report::msg(format!(
                                "missing required value for '--{}'",
                                flag.name
                            )));
                        }
                    }
                    FlagAction::Count => {
                        let vals = flag_matches.vals.last_mut().unwrap();
                        let count = vals.pop().unwrap().downcast_into::<u8>().unwrap();
                        vals.push(AnyValue::new(count + 1));
                        flag_matches.raw_vals.push(vec![Cow::Borrowed("")]);
                    }
                    FlagAction::Append => {
                        let value = argv
                            .pop_front()
                            .or(flag.default_missing_value.map(Cow::Borrowed))
                            .or(flag.default_value.map(Cow::Borrowed));
                        if let Some(value) = value {
                            flag_matches.raw_vals.push(vec![value]);
                        } else {
                            return Err(Report::msg(format!(
                                "missing required value for '--{}'",
                                flag.name
                            )));
                        }
                    }
                    FlagAction::SetTrue | FlagAction::SetFalse => {
                        let vals = flag_matches.vals.last_mut().unwrap();
                        vals.pop();
                        vals.push(AnyValue::new(flag.action.as_boolean_value()));
                        flag_matches.raw_vals.push(vec![Cow::Borrowed("")]);
                    }
                }
            }

            Ok(this)
        }

        pub fn iter(&self) -> impl Iterator<Item = (&'static str, &[Vec<Cow<'static, str>>])> + '_ {
            self.args.iter().map(|(k, matched)| (*k, matched.raw_vals.as_slice()))
        }

        fn register_flag(&mut self, flag: &CompileFlag) {
            assert!(
                !self.args.contains_key(flag.name),
                "command line flag {} is already registered",
                flag.name
            );

            #[cfg(debug_assertions)]
            {
                self.valid_args.push(flag.name);
            }

            let default_value = match flag.action {
                FlagAction::Count => {
                    vec![AnyValue::new(0u8)]
                }
                FlagAction::SetTrue => {
                    let default = flag
                        .default_value
                        .or(flag.default_missing_value)
                        .map(|default_value| default_value == "true")
                        .unwrap_or_default();
                    vec![AnyValue::new(default)]
                }
                FlagAction::SetFalse => {
                    vec![AnyValue::new(
                        flag.default_value
                            .or(flag.default_missing_value)
                            .map(|default_value| default_value == "true")
                            .unwrap_or(true),
                    )]
                }
                _ => vec![],
            };
            self.args.insert(
                flag.name,
                MatchedArg {
                    source: None,
                    indices: vec![],
                    type_id: None,
                    vals: vec![default_value],
                    raw_vals: vec![],
                    ignore_case: false,
                },
            );
        }

        fn append_value(&mut self, id: &'static str, val: AnyValue, raw: Cow<'static, str>) {
            let arg = self.args.get_mut(id).unwrap();
            arg.vals.last_mut().unwrap().push(val);
            arg.raw_vals.last_mut().unwrap().push(raw);
        }
    }

    #[derive(Default, Debug, Clone)]
    struct MatchedArg {
        source: Option<ValueSource>,
        indices: Vec<usize>,
        type_id: Option<AnyValueId>,
        vals: Vec<Vec<AnyValue>>,
        raw_vals: Vec<Vec<Cow<'static, str>>>,
        ignore_case: bool,
    }

    impl MatchedArg {
        pub fn first(&self) -> Option<&AnyValue> {
            self.vals.iter().flatten().next()
        }

        pub fn type_id(&self) -> Option<AnyValueId> {
            self.type_id
        }

        pub fn infer_type_id(&self, expected: AnyValueId) -> AnyValueId {
            self.type_id()
                .or_else(|| {
                    self.vals
                        .iter()
                        .flatten()
                        .map(|v| v.type_id())
                        .find(|actual| *actual != expected)
                })
                .unwrap_or(expected)
        }
    }

    impl ArgMatches {
        #[cfg_attr(debug_assertions, track_caller)]
        pub fn get_one<T: Any + Clone + Send + Sync + 'static>(&self, id: &str) -> Option<&T> {
            MatchesError::unwrap(id, self.try_get_one(id))
        }

        #[cfg_attr(debug_assertions, track_caller)]
        pub fn get_count(&self, id: &str) -> u8 {
            *self.get_one::<u8>(id).unwrap_or_else(|| {
                panic!("arg `{id}`'s `ArgAction` should be `Count` which should provide a default")
            })
        }

        #[cfg_attr(debug_assertions, track_caller)]
        pub fn get_flag(&self, id: &str) -> bool {
            *self.get_one::<bool>(id).unwrap_or_else(|| {
                panic!(
                    "arg `{id}`'s `ArgAction` should be one of `SetTrue`, `SetFalse` which should \
                     provide a default"
                )
            })
        }

        /// Non-panicking version of [`ArgMatches::get_one`]
        pub fn try_get_one<T: Any + Clone + Send + Sync + 'static>(
            &self,
            id: &str,
        ) -> Result<Option<&T>, MatchesError> {
            let arg = self.try_get_arg_t::<T>(id)?;
            let value = match arg.and_then(|a| a.first()) {
                Some(value) => value,
                None => {
                    return Ok(None);
                }
            };
            Ok(value.downcast_ref::<T>().map(Some).unwrap())
        }

        #[inline]
        fn try_get_arg_t<T: Any + Send + Sync + 'static>(
            &self,
            arg: &str,
        ) -> Result<Option<&MatchedArg>, MatchesError> {
            let arg = match self.try_get_arg(arg)? {
                Some(arg) => arg,
                None => {
                    return Ok(None);
                }
            };
            self.verify_arg_t::<T>(arg)?;
            Ok(Some(arg))
        }

        #[inline]
        fn try_get_arg(&self, arg: &str) -> Result<Option<&MatchedArg>, MatchesError> {
            self.verify_arg(arg)?;
            Ok(self.args.get(arg))
        }

        fn verify_arg_t<T: Any + Send + Sync + 'static>(
            &self,
            arg: &MatchedArg,
        ) -> Result<(), MatchesError> {
            let expected = AnyValueId::of::<T>();
            let actual = arg.infer_type_id(expected);
            if expected == actual {
                Ok(())
            } else {
                Err(MatchesError::Downcast { actual, expected })
            }
        }

        #[inline]
        fn verify_arg(&self, _arg: &str) -> Result<(), MatchesError> {
            #[cfg(debug_assertions)]
            {
                if _arg.is_empty() || self.valid_args.contains(&_arg) {
                } else {
                    log::debug!(
                        target: "driver",
                        "`{_arg:?}` is not an id of an argument or a group.\nMake sure you're using \
                         the name of the argument itself and not the name of short or long flags."
                    );
                    return Err(MatchesError::UnknownArgument {});
                }
            }
            Ok(())
        }
    }

    #[derive(Clone)]
    struct AnyValue {
        inner: Arc<dyn core::any::Any + Send + Sync + 'static>,
        id: AnyValueId,
    }

    impl AnyValue {
        fn new<V: core::any::Any + Clone + Send + Sync + 'static>(inner: V) -> Self {
            let id = AnyValueId::of::<V>();
            let inner = Arc::new(inner);
            Self { inner, id }
        }

        pub(crate) fn downcast_ref<T: core::any::Any + Clone + Send + Sync + 'static>(
            &self,
        ) -> Option<&T> {
            self.inner.downcast_ref::<T>()
        }

        pub(crate) fn downcast_into<T: core::any::Any + Clone + Send + Sync>(
            self,
        ) -> Result<T, Self> {
            let id = self.id;
            let value = Arc::downcast::<T>(self.inner).map_err(|inner| Self { inner, id })?;
            let value = Arc::try_unwrap(value).unwrap_or_else(|arc| (*arc).clone());
            Ok(value)
        }

        pub(crate) fn type_id(&self) -> AnyValueId {
            self.id
        }
    }

    impl core::fmt::Debug for AnyValue {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
            f.debug_struct("AnyValue").field("inner", &self.id).finish()
        }
    }

    #[derive(Copy, Clone)]
    pub struct AnyValueId {
        type_id: core::any::TypeId,
        #[cfg(debug_assertions)]
        type_name: &'static str,
    }

    impl AnyValueId {
        pub(crate) fn of<A: ?Sized + 'static>() -> Self {
            Self {
                type_id: core::any::TypeId::of::<A>(),
                #[cfg(debug_assertions)]
                type_name: core::any::type_name::<A>(),
            }
        }
    }

    impl PartialEq for AnyValueId {
        fn eq(&self, other: &Self) -> bool {
            self.type_id == other.type_id
        }
    }

    impl Eq for AnyValueId {}

    impl PartialOrd for AnyValueId {
        fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl PartialEq<core::any::TypeId> for AnyValueId {
        fn eq(&self, other: &core::any::TypeId) -> bool {
            self.type_id == *other
        }
    }

    impl Ord for AnyValueId {
        fn cmp(&self, other: &Self) -> core::cmp::Ordering {
            self.type_id.cmp(&other.type_id)
        }
    }

    impl core::hash::Hash for AnyValueId {
        fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
            self.type_id.hash(state);
        }
    }

    impl core::fmt::Debug for AnyValueId {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
            #[cfg(not(debug_assertions))]
            {
                self.type_id.fmt(f)
            }
            #[cfg(debug_assertions)]
            {
                f.debug_struct(self.type_name).finish()
            }
        }
    }

    impl<'a, A: ?Sized + 'static> From<&'a A> for AnyValueId {
        fn from(_: &'a A) -> Self {
            Self::of::<A>()
        }
    }

    /// Origin of the argument's value
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    #[non_exhaustive]
    pub enum ValueSource {
        /// Value came [`Arg::default_value`][crate::Arg::default_value]
        DefaultValue,
        /// Value came [`Arg::env`][crate::Arg::env]
        EnvVariable,
        /// Value was passed in on the command-line
        CommandLine,
    }

    impl ValueSource {
        pub(crate) fn is_explicit(self) -> bool {
            self != Self::DefaultValue
        }
    }
}
