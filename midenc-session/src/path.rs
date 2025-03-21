#[cfg(feature = "std")]
pub use std::path::{Path, PathBuf};

#[cfg(not(feature = "std"))]
pub use self::fallback::{Path, PathBuf};

#[cfg(not(feature = "std"))]
mod fallback {
    use alloc::{borrow::Cow, boxed::Box, vec::Vec};
    use core::{borrow::Borrow, fmt::Display, ops::Deref};

    #[derive(Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub struct PathBuf(Vec<u8>);

    impl PathBuf {
        pub const fn new() -> Self {
            Self(Vec::new())
        }

        #[inline]
        pub fn as_path(&self) -> &Path {
            // SAFETY: Path just wraps [u8] and &*self.0 is &[u8], which is safe to transmute to &Path
            unsafe { core::mem::transmute(&*self.0) }
        }

        #[inline]
        pub fn into_boxed_path(self) -> Box<Path> {
            unsafe { core::mem::transmute(self.0.into_boxed_slice()) }
        }
    }

    impl From<&str> for PathBuf {
        fn from(s: &str) -> Self {
            Self(s.as_bytes().to_vec())
        }
    }

    impl core::fmt::Debug for PathBuf {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let s = self.as_path().to_string_lossy();
            write!(f, "{s}")
        }
    }

    impl Deref for PathBuf {
        type Target = Path;

        #[inline]
        fn deref(&self) -> &Self::Target {
            self.as_path()
        }
    }

    impl AsRef<Path> for PathBuf {
        #[inline]
        fn as_ref(&self) -> &Path {
            self.as_path()
        }
    }

    impl Borrow<Path> for PathBuf {
        #[inline]
        fn borrow(&self) -> &Path {
            self.as_path()
        }
    }

    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    #[repr(transparent)]
    pub struct Path([u8]);

    impl core::fmt::Debug for Path {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let s = self.to_string_lossy();
            write!(f, "{s}")
        }
    }

    impl Path {
        pub fn to_str(&self) -> Option<&str> {
            core::str::from_utf8(&self.0).ok()
        }

        pub fn to_string_lossy(&self) -> Cow<'_, str> {
            alloc::string::String::from_utf8_lossy(&self.0)
        }

        pub fn to_path_buf(&self) -> PathBuf {
            PathBuf(self.0.to_vec())
        }

        pub fn display(&self) -> impl Display + '_ {
            self.to_string_lossy()
        }

        pub fn is_absolute(&self) -> bool {
            self.0.starts_with(b"/")
        }

        pub fn parent(&self) -> Option<&Self> {
            if self.0.is_empty() || self.0.ends_with(b"/") {
                return None;
            }
            match self.to_str() {
                None => match self.0.rsplit_once(|b| *b == b'/') {
                    None => Some(Self::from_bytes(&self.0)),
                    Some((before, _)) => Some(Self::from_bytes(before)),
                },
                Some(s) => match s.rsplit_once('/') {
                    None => Some(s.as_ref()),
                    Some((before, _)) => Some(before.as_ref()),
                },
            }
        }

        pub fn file_name(&self) -> Option<&Self> {
            if self.0.ends_with(b"..") {
                return None;
            }
            match self.to_str() {
                None => match self.0.rsplit_once(|b| *b == b'/') {
                    None => Some(Self::from_bytes(&self.0)),
                    Some((_, after)) => Some(Self::from_bytes(after)),
                },
                Some(s) => match s.rsplit_once('/') {
                    None => Some(s.as_ref()),
                    Some((_, after)) => Some(after.as_ref()),
                },
            }
        }

        pub fn file_stem(&self) -> Option<&Self> {
            let file_name = self.file_name()?;
            match file_name.0.rsplit_once(|b| *b == b'.') {
                None => Some(file_name),
                Some(([], _)) => Some(file_name),
                Some((stem, _)) => Some(Self::from_bytes(stem)),
            }
        }

        pub fn extension(&self) -> Option<&Self> {
            let file_name = self.file_name()?;
            match file_name.0.rsplit_once(|b| *b == b'.') {
                None => None,
                Some(([], _)) => None,
                Some((_, ext)) => Some(Self::from_bytes(ext)),
            }
        }

        pub fn is_dir(&self) -> bool {
            self.extension().is_none()
        }

        pub fn join<P>(&self, path: P) -> PathBuf
        where
            P: AsRef<Path>,
        {
            let path = path.as_ref();
            if self.0.is_empty() {
                return path.to_path_buf();
            }

            let mut buf = Vec::with_capacity(self.0.len() + path.0.len() + 1);
            buf.extend_from_slice(&self.0);
            buf.push(b'/');
            buf.extend_from_slice(&path.0);

            PathBuf(buf)
        }

        pub fn with_stem<S>(&self, stem: S) -> PathBuf
        where
            S: AsRef<str>,
        {
            let stem = stem.as_ref().as_bytes();
            match self.file_name() {
                None => {
                    let mut buf = Vec::with_capacity(self.0.len() + stem.len());
                    buf.extend_from_slice(&self.0);
                    buf.extend_from_slice(stem);
                    PathBuf(buf)
                }
                Some(file_name) => {
                    let (prefix, ext) = match file_name.0.rsplit_once(|b| *b == b'.') {
                        None => (self.0.strip_suffix(&file_name.0).unwrap(), None),
                        Some((name, ext)) => {
                            let len = self.0.len() - name.len() - 1 - ext.len();
                            (&self.0[..len], Some(ext))
                        }
                    };
                    let mut buf = Vec::with_capacity(
                        prefix.len() + ext.map(|ext| ext.len()).unwrap_or_default() + stem.len(),
                    );
                    buf.extend_from_slice(prefix);
                    buf.extend_from_slice(stem);
                    if let Some(ext) = ext {
                        buf.push(b'.');
                        buf.extend_from_slice(ext)
                    }
                    PathBuf(buf)
                }
            }
        }

        pub fn with_extension<S>(&self, extension: S) -> PathBuf
        where
            S: AsRef<str>,
        {
            let extension = extension.as_ref().as_bytes();
            match self.extension() {
                None => {
                    let mut buf = self.to_path_buf();
                    buf.0.push(b'.');
                    buf.0.extend_from_slice(extension);
                    buf
                }
                Some(prev) => {
                    let bytes = self.0.strip_suffix(&prev.0).unwrap();
                    let mut buf = Vec::with_capacity(bytes.len() + extension.len());
                    buf.extend_from_slice(bytes);
                    buf.extend_from_slice(extension);
                    PathBuf(buf)
                }
            }
        }

        pub fn with_stem_and_extension<S, E>(&self, stem: S, extension: E) -> PathBuf
        where
            S: AsRef<str>,
            E: AsRef<str>,
        {
            let stem = stem.as_ref().as_bytes();
            let extension = extension.as_ref().as_bytes();
            match self.file_name() {
                None => {
                    let mut buf =
                        Vec::with_capacity(self.0.len() + stem.len() + 1 + extension.len());
                    buf.extend_from_slice(&self.0);
                    buf.extend_from_slice(stem);
                    buf.push(b'.');
                    buf.extend_from_slice(extension);
                    PathBuf(buf)
                }
                Some(file_name) => {
                    let bytes = self.0.strip_suffix(&file_name.0).unwrap();
                    let mut buf =
                        Vec::with_capacity(bytes.len() + stem.len() + 1 + extension.len());
                    buf.extend_from_slice(bytes);
                    buf.extend_from_slice(stem);
                    buf.push(b'.');
                    buf.extend_from_slice(extension);
                    PathBuf(buf)
                }
            }
        }

        #[inline(always)]
        fn from_bytes(bytes: &[u8]) -> &Self {
            unsafe { core::mem::transmute(bytes) }
        }
    }

    impl AsRef<Path> for str {
        fn as_ref(&self) -> &Path {
            unsafe { core::mem::transmute(self.as_bytes()) }
        }
    }

    impl AsRef<Path> for alloc::string::String {
        fn as_ref(&self) -> &Path {
            unsafe { core::mem::transmute(self.as_bytes()) }
        }
    }

    impl Clone for Box<Path> {
        fn clone(&self) -> Self {
            self.to_path_buf().into_boxed_path()
        }
    }
}
