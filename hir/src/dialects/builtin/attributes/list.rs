use core::fmt;

use crate::{
    AttrPrinter, Attribute, OpPrintingFlags, attributes::AttrList, derive::DialectAttribute,
    dialects::builtin::BuiltinDialect, print::AsmPrinter,
};

#[derive(DialectAttribute, Default)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
pub struct List(AttrList);

impl fmt::Debug for List {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        for entry in self.0.iter() {
            let attr = entry.as_trait::<dyn Attribute>().unwrap();
            list.entry_with(|f| write!(f, "{attr:?}"));
        }
        list.finish()
    }
}

impl fmt::Display for List {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        let flags = OpPrintingFlags::default();
        for entry in self.0.iter() {
            let attr = entry.as_trait::<dyn Attribute>().unwrap();
            list.entry_with(|f| {
                let context = attr.context_rc();
                let mut printer = AsmPrinter::new(context, &flags);
                printer.print_attribute_value(attr);
                write!(f, "{}", printer.finish())
            });
        }
        list.finish()
    }
}

impl Eq for List {}
impl PartialEq for List {
    fn eq(&self, other: &Self) -> bool {
        let mut lhs = self.0.front();
        let mut rhs = other.0.front();
        loop {
            match (lhs.get(), rhs.get()) {
                (None, None) => break true,
                (Some(l), Some(r)) => {
                    if !l
                        .as_trait::<dyn Attribute>()
                        .unwrap()
                        .dyn_eq(r.as_trait::<dyn Attribute>().unwrap())
                    {
                        break false;
                    }
                }
                _ => break false,
            }

            lhs.move_next();
            rhs.move_next();
        }
    }
}

impl Clone for List {
    fn clone(&self) -> Self {
        let mut list = AttrList::new();
        for attr in self.0.iter() {
            let cloned = attr.as_trait::<dyn Attribute>().unwrap().dyn_clone();
            let attr = cloned.borrow().as_attr().as_attr_ref();
            list.push_back(attr);
        }
        Self(list)
    }
}

impl core::hash::Hash for List {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        for attr in self.0.iter() {
            attr.as_trait::<dyn Attribute>().unwrap().dyn_hash(state);
        }
    }
}

impl From<AttrList> for List {
    fn from(value: AttrList) -> Self {
        Self(value)
    }
}

impl From<List> for AttrList {
    fn from(value: List) -> Self {
        value.0
    }
}

impl AsRef<AttrList> for List {
    fn as_ref(&self) -> &AttrList {
        &self.0
    }
}

impl AsMut<AttrList> for List {
    fn as_mut(&mut self) -> &mut AttrList {
        &mut self.0
    }
}

impl AttrPrinter for ListAttr {
    fn print(&self, _printer: &mut AsmPrinter<'_>) {
        todo!()
    }
}
