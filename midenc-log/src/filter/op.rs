use std::fmt;

#[derive(Debug, Clone)]
pub(crate) struct FilterOp {
    #[cfg(feature = "regex")]
    inner: regex::Regex,
    #[cfg(not(feature = "regex"))]
    inner: String,
}

impl Eq for FilterOp {}
impl PartialEq for FilterOp {
    #[cfg(feature = "regex")]
    fn eq(&self, other: &Self) -> bool {
        self.inner.as_str() == other.inner.as_str()
    }

    #[cfg(not(feature = "regex"))]
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl PartialOrd for FilterOp {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FilterOp {
    #[cfg(feature = "regex")]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.inner
            .as_str()
            .len()
            .cmp(&other.inner.as_str().len())
            .then_with(|| self.inner.as_str().cmp(other.inner.as_str()))
    }

    #[cfg(not(feature = "regex"))]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.inner
            .len()
            .cmp(&other.inner.len())
            .then_with(|| self.inner.cmp(&other.inner))
    }
}

#[cfg(feature = "regex")]
impl FilterOp {
    pub(crate) fn new(spec: &str) -> Result<Self, String> {
        match regex::Regex::new(spec) {
            Ok(r) => Ok(Self { inner: r }),
            Err(e) => Err(e.to_string()),
        }
    }

    pub(crate) fn is_match(&self, s: &str) -> bool {
        self.inner.is_match(s)
    }

    pub(crate) fn as_str(&self) -> &str {
        self.inner.as_str()
    }
}

#[cfg(not(feature = "regex"))]
impl FilterOp {
    pub fn new(spec: &str) -> Result<Self, String> {
        Ok(Self {
            inner: spec.to_string(),
        })
    }

    pub fn is_match(&self, s: &str) -> bool {
        s.contains(&self.inner)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.inner
    }
}

impl fmt::Display for FilterOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}
