use std::{
    ffi::{OsStr, OsString},
    fmt,
    str::FromStr,
};

use clap::{Parser, ValueEnum};
use midenc_codegen_masm::NativePtr;
use midenc_hir::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadMemoryExpr {
    pub addr: NativePtr,
    pub ty: Type,
    pub count: u8,
    pub mode: MemoryMode,
    pub format: FormatType,
}
impl FromStr for ReadMemoryExpr {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let argv = s.split_whitespace();
        let args = Read::parse(argv)?;

        let ty = args.ty.unwrap_or_else(|| Type::Array(Box::new(Type::Felt), 4));
        let addr = match args.mode {
            MemoryMode::Word => NativePtr::new(args.addr, 0, 0),
            MemoryMode::Byte => NativePtr::from_ptr(args.addr),
        };
        Ok(Self {
            addr,
            ty,
            count: args.count,
            mode: args.mode,
            format: args.format,
        })
    }
}

#[derive(Default, Debug, Parser)]
#[command(name = "read")]
pub struct Read {
    /// The memory address to start reading from
    #[arg(required(true), value_name = "ADDR", value_parser(parse_address))]
    pub addr: u32,
    /// The type of value to read from ADDR, defaults to 'word'
    #[arg(
        short = 't',
        long = "type",
        value_name = "TYPE",
        value_parser(TypeParser)
    )]
    pub ty: Option<Type>,
    /// The number of values to read
    #[arg(short = 'c', long = "count", value_name = "N", default_value_t = 1)]
    pub count: u8,
    /// The addressing mode to use
    #[arg(
        short = 'm',
        long = "mode",
        value_name = "MODE",
        default_value_t = MemoryMode::Word,
        value_parser(MemoryModeParser)
    )]
    pub mode: MemoryMode,
    /// The format to use when printing integral values
    #[arg(
        short = 'f',
        long = "format",
        value_name = "FORMAT",
        default_value_t = FormatType::Decimal,
        value_parser(FormatTypeParser)
    )]
    pub format: FormatType,
}
impl Read {
    pub fn parse<I, S>(argv: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString> + Clone,
    {
        let command = <Self as clap::CommandFactory>::command()
            .disable_help_flag(true)
            .disable_version_flag(true)
            .disable_colored_help(true)
            .no_binary_name(true);

        let mut matches = command.try_get_matches_from(argv).map_err(|err| err.to_string())?;
        <Self as clap::FromArgMatches>::from_arg_matches_mut(&mut matches)
            .map_err(|err| err.to_string())
    }
}

#[doc(hidden)]
#[derive(Clone)]
struct TypeParser;
impl clap::builder::TypedValueParser for TypeParser {
    type Value = Type;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::error::Error> {
        use clap::error::{Error, ErrorKind};

        let value = value.to_str().ok_or_else(|| Error::new(ErrorKind::InvalidUtf8))?;

        Ok(match value {
            "i1" => Type::I1,
            "i8" => Type::I8,
            "i16" => Type::I16,
            "i32" => Type::I32,
            "i64" => Type::I64,
            "i128" => Type::I128,
            "u8" => Type::U8,
            "u16" => Type::U16,
            "u32" => Type::U32,
            "u64" => Type::U64,
            "u128" => Type::U128,
            "felt" => Type::Felt,
            "word" => Type::Array(Box::new(Type::Felt), 4),
            "ptr" | "pointer" => Type::Ptr(Box::new(Type::U32)),
            _ => {
                return Err(Error::raw(
                    ErrorKind::InvalidValue,
                    format!("invalid/unsupported type '{value}'"),
                ))
            }
        })
    }
}

fn parse_address(s: &str) -> Result<u32, String> {
    if let Some(s) = s.strip_prefix("0x") {
        u32::from_str_radix(s, 16).map_err(|err| format!("invalid memory address: {err}"))
    } else if s.is_empty() {
        Err(format!("expected memory address at '{s}'"))
    } else {
        s.parse::<u32>().map_err(|err| format!("invalid memory address: {err}"))
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum MemoryMode {
    #[default]
    Word,
    Byte,
}
impl fmt::Display for MemoryMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Word => f.write_str("word"),
            Self::Byte => f.write_str("byte"),
        }
    }
}
impl FromStr for MemoryMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "w" | "word" | "words" | "miden" => Ok(Self::Word),
            "b" | "byte" | "bytes" | "rust" => Ok(Self::Byte),
            _ => Err(format!("invalid memory mode '{s}'")),
        }
    }
}

#[doc(hidden)]
#[derive(Clone)]
struct MemoryModeParser;
impl clap::builder::TypedValueParser for MemoryModeParser {
    type Value = MemoryMode;

    fn possible_values(
        &self,
    ) -> Option<Box<dyn Iterator<Item = clap::builder::PossibleValue> + '_>> {
        use clap::builder::PossibleValue;
        Some(Box::new(
            [
                PossibleValue::new("words").aliases(["w", "word", "miden"]),
                PossibleValue::new("bytes").aliases(["b", "byte", "rust"]),
            ]
            .into_iter(),
        ))
    }

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::error::Error> {
        use clap::error::{Error, ErrorKind};

        let value = value.to_str().ok_or_else(|| Error::new(ErrorKind::InvalidUtf8))?;
        value.parse().map_err(|err| Error::raw(ErrorKind::InvalidValue, err))
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum FormatType {
    #[default]
    Decimal,
    Hex,
    Binary,
}
impl fmt::Display for FormatType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decimal => f.write_str("decimal"),
            Self::Hex => f.write_str("hex"),
            Self::Binary => f.write_str("binary"),
        }
    }
}
impl FromStr for FormatType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "d" | "decimal" => Ok(Self::Decimal),
            "x" | "hex" | "hexadecimal" => Ok(Self::Hex),
            "b" | "bin" | "binary" | "bits" => Ok(Self::Binary),
            _ => Err(format!("invalid format type '{s}'")),
        }
    }
}

#[doc(hidden)]
#[derive(Clone)]
struct FormatTypeParser;
impl clap::builder::TypedValueParser for FormatTypeParser {
    type Value = FormatType;

    fn possible_values(
        &self,
    ) -> Option<Box<dyn Iterator<Item = clap::builder::PossibleValue> + '_>> {
        use clap::builder::PossibleValue;
        Some(Box::new(
            [
                PossibleValue::new("decimal").alias("d"),
                PossibleValue::new("hex").aliases(["x", "hexadecimal"]),
                PossibleValue::new("binary").aliases(["b", "bin", "bits"]),
            ]
            .into_iter(),
        ))
    }

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::error::Error> {
        use clap::error::{Error, ErrorKind};

        let value = value.to_str().ok_or_else(|| Error::new(ErrorKind::InvalidUtf8))?;
        value.parse().map_err(|err| Error::raw(ErrorKind::InvalidValue, err))
    }
}
