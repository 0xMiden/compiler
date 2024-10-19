use core::hash::{Hash, Hasher};

/// A type-erased version of [core::hash::Hash]
pub trait DynHash {
    fn dyn_hash(&self, hasher: &mut dyn Hasher);
}

impl<H: Hash> DynHash for H {
    #[inline]
    fn dyn_hash(&self, hasher: &mut dyn Hasher) {
        let mut hasher = DynHasher(hasher);
        <Self as Hash>::hash(self, &mut hasher)
    }
}

pub struct DynHasher<'a>(&'a mut dyn Hasher);

impl<'a> DynHasher<'a> {
    pub fn new<H>(hasher: &'a mut H) -> Self
    where
        H: Hasher,
    {
        Self(hasher)
    }
}

impl<'a> Hasher for DynHasher<'a> {
    #[inline]
    fn finish(&self) -> u64 {
        self.0.finish()
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes)
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.0.write_u8(i);
    }

    #[inline]
    fn write_i8(&mut self, i: i8) {
        self.0.write_i8(i);
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.0.write_u16(i);
    }

    #[inline]
    fn write_i16(&mut self, i: i16) {
        self.0.write_i16(i);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.0.write_u32(i);
    }

    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.0.write_i32(i);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0.write_u64(i);
    }

    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.0.write_i64(i);
    }

    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.0.write_u128(i);
    }

    #[inline]
    fn write_i128(&mut self, i: i128) {
        self.0.write_i128(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.0.write_usize(i);
    }

    #[inline]
    fn write_isize(&mut self, i: isize) {
        self.0.write_isize(i);
    }
}
