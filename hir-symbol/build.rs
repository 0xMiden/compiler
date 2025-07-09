extern crate hashbrown;
extern crate inflector;
extern crate rustc_hash;
extern crate toml;

use std::{
    cmp::Ordering,
    collections::BTreeSet,
    env,
    fs::{self, File},
    io::prelude::*,
    path::{Path, PathBuf},
};

use inflector::Inflector;
use toml::{value::Table, Value};

type FxHashSet<K> = hashbrown::HashSet<K, rustc_hash::FxBuildHasher>;

#[derive(Default)]
struct SymbolMap {
    sections: Vec<Section>,
}

impl SymbolMap {
    pub fn sections(&self) -> impl Iterator<Item = &Section> {
        self.sections.iter()
    }
}

impl core::str::FromStr for SymbolMap {
    type Err = toml::de::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use serde::de::Error as _;
        use toml::de::Error;

        let mut this = Self::default();
        let map = toml::from_str::<toml::Table>(s)?;

        for (k, symbols) in map.into_iter() {
            let symbols = symbols.as_table().ok_or_else(|| {
                Error::custom(format!("invalid symbol section '{k}': expected table"))
            })?;
            let section = Section::from_table(k, symbols);
            this.sections.push(section);
        }

        Ok(this)
    }
}

struct Section {
    name: String,
    keys: BTreeSet<Symbol>,
}
impl Section {
    fn new(name: String) -> Self {
        Self {
            name,
            keys: BTreeSet::new(),
        }
    }

    fn from_table(name: String, table: &Table) -> Self {
        let mut section = Section::new(name);
        for (name, value) in table.iter() {
            let mut sym = Symbol::from_value(name, value);
            sym.is_keyword = section.name == "keywords";
            assert!(section.keys.insert(sym), "duplicate symbol {}", name);
        }
        section
    }

    fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.keys.iter()
    }
}

#[derive(Debug, Default, Clone)]
struct Symbol {
    key: String,
    id: Option<i64>,
    value: String,
    is_keyword: bool,
}
impl Eq for Symbol {}
impl PartialEq for Symbol {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}
impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> Ordering {
        // Ensure that we always consider symbols with the same value equivalent
        if self.value == other.value {
            return Ordering::Equal;
        }

        // Otherwise, sort by id first, then value
        match (self.id, other.id) {
            (None, None) => self.value.cmp(&other.value),
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => x.cmp(&y),
            (None, Some(_)) => Ordering::Greater,
        }
    }
}
impl Symbol {
    fn from_value<S: Into<String>>(name: S, value: &Value) -> Self {
        let name = name.into();
        let table = value.as_table().unwrap();
        let id = table.get("id").map(|id| id.as_integer().expect("id must be an integer"));
        let value = match table.get("value").map(|v| v.as_str().expect("value must be a string")) {
            None => name.clone(),
            Some(value) => value.to_string(),
        };
        // When the name is, e.g. UPPER_CASE, keep it that way rather than transforming it
        // as the casing is intentional
        let key = if name.is_screaming_snake_case() {
            name
        } else {
            name.to_pascal_case()
        };
        Self {
            key,
            id,
            value,
            is_keyword: false,
        }
    }
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_file = out_dir.join("symbols.rs");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/symbols.toml");
    println!("cargo:rustc-env=SYMBOLS_RS={}", out_file.display());

    let contents = fs::read_to_string("src/symbols.toml").unwrap();
    let mut symbol_map = contents.parse::<SymbolMap>().unwrap();

    let mut reserved = FxHashSet::default();
    for section in symbol_map.sections() {
        for symbol in section.iter() {
            if let Some(id) = symbol.id {
                assert!(
                    reserved.insert(id),
                    "duplicate symbol id {} in section {}",
                    id,
                    &section.name
                );
            }
        }
    }

    let mut symbols = vec![];
    symbol_map.sections.drain(..).fold(0, |next_id, section| {
        section.keys.iter().fold(next_id, |mut next_id, symbol| {
            let mut symbol = symbol.clone();
            while reserved.contains(&next_id) {
                next_id += 1;
            }
            if symbol.id.is_none() {
                symbol.id = Some(next_id);
                next_id += 1
            }
            symbols.push(symbol);
            next_id
        })
    });

    generate_symbols_rs(&out_file, symbols).unwrap();
}

fn generate_symbols_rs(path: &Path, symbols: Vec<Symbol>) -> std::io::Result<()> {
    let mut file = File::create(path)?;

    // Symbol declarations
    for symbol in symbols.iter() {
        let key = &symbol.key;
        let id = symbol.id.unwrap();
        writeln!(&mut file, "#[allow(non_upper_case_globals)]")?;
        writeln!(&mut file, "pub const {key}: crate::Symbol = crate::Symbol::new({id});")?
    }

    // Symbol strings
    file.write_all(b"\n\npub(crate) const __SYMBOLS: &[(crate::Symbol, &str)] = &[\n")?;
    for symbol in symbols.iter() {
        let key = &symbol.key;
        let value = &symbol.value;
        writeln!(&mut file, "    ({key}, \"{value}\"),")?;
    }
    file.write_all(b"];\n\n")?;

    // fn is_keyword(sym: Symbol) -> bool
    file.write_all(b"pub fn is_keyword(sym: crate::Symbol) -> bool {\n")?;
    file.write_all(b"    #[allow(non_upper_case_globals, clippy::match_like_matches_macro)]\n")?;
    file.write_all(b"    match sym {\n")?;
    for symbol in symbols.iter().filter(|s| s.is_keyword) {
        let key = &symbol.key;
        writeln!(&mut file, "        {key} => true,")?;
    }
    file.write_all(b"        _ => false,\n")?;
    file.write_all(b"    }\n}\n\n")?;

    Ok(())
}
