use core::fmt;

use midenc_hir2::{formatter::PrettyPrint, FunctionIdent, Ident, Signature};

#[derive(Debug)]
pub struct SyntheticFunction {
    pub id: FunctionIdent,
    pub signature: Signature,
    pub inner_function: FunctionIdent,
}

impl fmt::Display for SyntheticFunction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

impl PrettyPrint for SyntheticFunction {
    fn render(&self) -> midenc_hir2::formatter::Document {
        use midenc_hir2::formatter::*;

        const_text("(")
            + const_text("synth_func")
            + const_text(" ")
            + display(self.id)
            + const_text(" ")
            + self.signature.render()
            + const_text(" (inner ")
            + self.inner_function.render()
            + const_text(")")
            + const_text(")")
    }
}

#[derive(Debug)]
pub struct Interface {
    pub name: String,
    pub functions: Vec<SyntheticFunction>,
}

impl fmt::Display for Interface {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

impl PrettyPrint for Interface {
    fn render(&self) -> midenc_hir2::formatter::Document {
        use midenc_hir2::formatter::*;

        let functions = self
            .functions
            .iter()
            .map(PrettyPrint::render)
            .reduce(|acc, doc| acc + nl() + doc)
            .unwrap_or(Document::Empty);

        const_text("(")
            + const_text("interface")
            + const_text(" ")
            + text(&self.name)
            + indent(4, nl() + functions)
            + nl()
            + const_text(")")
    }
}

#[derive(Debug)]
pub struct Module {
    pub name: Ident,
    pub functions: Vec<SyntheticFunction>,
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

impl PrettyPrint for Module {
    fn render(&self) -> midenc_hir2::formatter::Document {
        use midenc_hir2::formatter::*;

        let functions = self
            .functions
            .iter()
            .map(PrettyPrint::render)
            .reduce(|acc, doc| acc + nl() + doc)
            .unwrap_or(Document::Empty);

        const_text("(")
            + const_text("module")
            + const_text(" ")
            + display(self.name)
            + indent(4, nl() + functions)
            + nl()
            + const_text(")")
    }
}

#[derive(Debug, Default)]
pub struct Component {
    pub name: String,
    pub interfaces: Vec<Interface>,
    pub modules: Vec<Module>,
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

impl PrettyPrint for Component {
    fn render(&self) -> midenc_hir2::formatter::Document {
        use midenc_hir2::formatter::*;

        let interfaces = self
            .interfaces
            .iter()
            .map(PrettyPrint::render)
            .reduce(|acc, doc| acc + nl() + doc)
            .map(|doc| const_text(";; Interfaces") + nl() + doc)
            .unwrap_or(Document::Empty);

        let modules = self
            .modules
            .iter()
            .map(PrettyPrint::render)
            .reduce(|acc, doc| acc + nl() + doc)
            .map(|doc| const_text(";; Modules") + nl() + doc)
            .unwrap_or(Document::Empty);

        let body = vec![interfaces, modules]
            .into_iter()
            .filter(|section| !section.is_empty())
            .fold(nl(), |a, b| {
                if matches!(a, Document::Newline) {
                    indent(4, a + b)
                } else {
                    a + nl() + indent(4, nl() + b)
                }
            });

        const_text("(")
            + const_text("component")
            + const_text(" ")
            + text(&self.name)
            + body
            + nl()
            + const_text(")")
            + nl()
    }
}

#[derive(Debug, Default)]
pub struct World {
    pub components: Vec<Component>,
}

impl fmt::Display for World {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}

impl PrettyPrint for World {
    fn render(&self) -> midenc_hir2::formatter::Document {
        use midenc_hir2::formatter::*;

        let components = self
            .components
            .iter()
            .map(PrettyPrint::render)
            .reduce(|acc, doc| acc + nl() + doc)
            .unwrap_or(Document::Empty);

        const_text("(")
            + const_text("world")
            + indent(4, nl() + components)
            + nl()
            + const_text(")")
            + nl()
    }
}

pub struct WorldBuilder {
    root: Component,
    imports: Vec<Component>,
}

impl WorldBuilder {
    pub fn new(name: String) -> Self {
        Self {
            root: Component {
                name,
                interfaces: vec![],
                modules: vec![],
            },
            imports: vec![],
        }
    }

    pub fn root_mut(&mut self) -> &mut Component {
        &mut self.root
    }

    pub fn add_import(&mut self, component: Component) {
        self.imports.push(component);
    }

    pub fn build(self) -> World {
        let mut components = Vec::with_capacity(1 + self.imports.len());
        components.extend(self.imports);
        components.push(self.root);
        World { components }
    }
}
