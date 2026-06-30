use crate::effects::{AdviceEffect, MemoryEffect};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemoryEffectDescriptor {
    pub effect: MemoryEffect,
    pub argument: Option<u8>,
    pub result: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AdviceEffectDescriptor {
    pub effect: AdviceEffect,
    pub resource: AdviceResourceKind,
    pub argument: Option<u8>,
    pub result: Option<u8>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum AdviceResourceKind {
    Map,
    Stack,
    MerkleStore,
}

impl AsRef<str> for AdviceResourceKind {
    fn as_ref(&self) -> &str {
        match self {
            Self::Map => "advice-map",
            Self::Stack => "advice-stack",
            Self::MerkleStore => "advice-merkle-store",
        }
    }
}

impl core::fmt::Display for AdviceResourceKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl core::str::FromStr for AdviceResourceKind {
    type Err = alloc::string::String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use alloc::string::ToString;
        match s {
            "advice-map" => Ok(Self::Map),
            "advice-stack" => Ok(Self::Stack),
            "advice-merkle-store" => Ok(Self::MerkleStore),
            s => Err(s.to_string()),
        }
    }
}
