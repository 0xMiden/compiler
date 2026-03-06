use super::*;

#[derive(Clone)]
pub struct SetAttr<K> {
    values: Vec<K>,
}
impl<K> Default for SetAttr<K> {
    fn default() -> Self {
        Self {
            values: Default::default(),
        }
    }
}
impl<K> SetAttr<K>
where
    K: Ord + Clone,
{
    pub fn insert(&mut self, key: K) -> bool {
        match self.values.binary_search_by(|k| key.cmp(k)) {
            Ok(index) => {
                self.values[index] = key;
                false
            }
            Err(index) => {
                self.values.insert(index, key);
                true
            }
        }
    }

    pub fn contains(&self, key: &K) -> bool {
        self.values.binary_search_by(|k| key.cmp(k)).is_ok()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, K> {
        self.values.iter()
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<K>
    where
        K: Borrow<Q>,
        Q: ?Sized + Ord,
    {
        match self.values.binary_search_by(|k| key.cmp(k.borrow())) {
            Ok(index) => Some(self.values.remove(index)),
            Err(_) => None,
        }
    }
}
impl<K> Eq for SetAttr<K> where K: Eq {}
impl<K> PartialEq for SetAttr<K>
where
    K: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}
impl<K> fmt::Debug for SetAttr<K>
where
    K: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.values.iter()).finish()
    }
}
impl<K> crate::formatter::PrettyPrint for SetAttr<K>
where
    K: crate::formatter::PrettyPrint,
{
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        let entries = self.values.iter().fold(Document::Empty, |acc, k| match acc {
            Document::Empty => k.render(),
            _ => acc + const_text(", ") + k.render(),
        });
        if self.values.is_empty() {
            const_text("{}")
        } else {
            const_text("{") + entries + const_text("}")
        }
    }
}
impl<K> crate::print::AttrPrinter for SetAttr<K>
where
    K: crate::formatter::PrettyPrint,
{
    fn print(
        &self,
        _flags: &crate::OpPrintingFlags,
        _context: &crate::Context,
    ) -> crate::formatter::Document {
        use crate::formatter::PrettyPrint;
        self.render()
    }
}
impl<K> core::hash::Hash for SetAttr<K>
where
    K: core::hash::Hash,
{
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        <Vec<K> as core::hash::Hash>::hash(&self.values, state);
    }
}
impl<K> AttributeValue for SetAttr<K>
where
    K: fmt::Debug + crate::formatter::PrettyPrint + Clone + Eq + core::hash::Hash + 'static,
{
    #[inline(always)]
    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }

    #[inline(always)]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self as &mut dyn Any
    }

    #[inline]
    fn clone_value(&self) -> Box<dyn AttributeValue> {
        Box::new(self.clone())
    }
}
