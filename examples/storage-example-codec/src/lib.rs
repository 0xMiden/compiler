//! Author-side codecs for the storage-example account storage slots.

use miden_codec::{AuthorTypeCodec, asset::AssetAmount};

miden_codec::from_project!("../storage-example");

#[miden_codec::codec]
impl AuthorTypeCodec for Limits {
    fn parse(value: &str) -> Result<Self, String> {
        let (max_qty, paused) = parse_flagged(value, "paused")?;
        Ok(Self { max_qty, paused })
    }

    fn display(&self) -> String {
        display_flagged(self.max_qty, self.paused, "paused")
    }

    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[miden_codec::codec]
impl AuthorTypeCodec for Holding {
    fn parse(value: &str) -> Result<Self, String> {
        let (qty, locked) = parse_flagged(value, "locked")?;
        Ok(Self { qty, locked })
    }

    fn display(&self) -> String {
        display_flagged(self.qty, self.locked, "locked")
    }

    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

miden_codec::export_codecs!();

/// Parses `"<amount>"` or `"<amount> <flag>"` into the amount and whether the flag is present.
fn parse_flagged(value: &str, flag: &str) -> Result<(AssetAmount, bool), String> {
    let mut words = value.split_whitespace();
    let amount = words.next().ok_or_else(|| "expected an amount".to_owned())?;
    let amount = amount
        .parse::<u64>()
        .map_err(|error| format!("invalid amount `{amount}`: {error}"))?;
    let amount = AssetAmount::new(amount).map_err(|error| error.to_string())?;
    let flagged = match words.next() {
        None => false,
        Some(word) if word == flag => true,
        Some(word) => return Err(format!("expected `{flag}` after the amount, found `{word}`")),
    };
    if let Some(word) = words.next() {
        return Err(format!("unexpected `{word}` after `{flag}`"));
    }
    Ok((amount, flagged))
}

/// Displays the amount, followed by the flag when it is set.
fn display_flagged(amount: AssetAmount, flagged: bool, flag: &str) -> String {
    if flagged {
        format!("{amount} {flag}")
    } else {
        amount.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_round_trip() {
        for text in ["100", "100 paused"] {
            assert_eq!(Limits::parse(text).unwrap().display(), text);
        }
        assert!(Limits::parse("100 locked").is_err());
    }

    #[test]
    fn holding_round_trip() {
        let holding = Holding::parse("7 locked").unwrap();
        assert!(holding.locked);
        assert_eq!(holding.display(), "7 locked");
        assert_eq!(Holding::parse(" 7 ").unwrap().display(), "7");
        assert!(Holding::parse("").is_err());
        assert!(Holding::parse("7 locked now").is_err());
    }
}
