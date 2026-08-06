//! Author-side codecs for the DEX note storage schema.

use miden_note_codec::AuthorTypeCodec;

miden_note_codec::from_project!("../dex-note");

#[miden_note_codec::note_codec]
impl AuthorTypeCodec for LimitPrice {
    fn parse(value: &str) -> Result<Self, String> {
        parse_limit_price(value)
    }

    fn display(&self) -> String {
        display_limit_price(self)
    }

    fn validate(&self) -> Result<(), String> {
        if self.denominator == 0 {
            Err("the limit-price denominator must not be zero".to_owned())
        } else {
            Ok(())
        }
    }
}

miden_note_codec::export_codecs!();

/// Parses a fraction or finite decimal limit price.
fn parse_limit_price(value: &str) -> Result<LimitPrice, String> {
    let (numerator, denominator) = if let Some((numerator, denominator)) = value.split_once('/') {
        (parse_part(numerator, "numerator")?, parse_part(denominator, "denominator")?)
    } else if let Some((whole, fraction)) = value.split_once('.') {
        if whole.is_empty() || fraction.is_empty() || fraction.contains('.') {
            return Err(format!("invalid decimal limit price `{value}`"));
        }
        let whole = parse_part(whole, "whole part")?;
        let fraction_value = parse_part(fraction, "fractional part")?;
        let scale = u32::try_from(fraction.len())
            .map_err(|_| format!("decimal limit price `{value}` has too many digits"))?;
        let denominator = 10_u64
            .checked_pow(scale)
            .ok_or_else(|| format!("decimal limit price `{value}` has too many digits"))?;
        let numerator = whole
            .checked_mul(denominator)
            .and_then(|whole| whole.checked_add(fraction_value))
            .ok_or_else(|| format!("decimal limit price `{value}` is too large"))?;
        (numerator, denominator)
    } else {
        (parse_part(value, "value")?, 1)
    };
    let divisor = greatest_common_divisor(numerator, denominator);
    let (numerator, denominator) = if divisor > 1 {
        (numerator / divisor, denominator / divisor)
    } else {
        (numerator, denominator)
    };
    Ok(LimitPrice {
        numerator,
        denominator,
    })
}

/// Parses one unsigned integer part.
fn parse_part(value: &str, name: &str) -> Result<u64, String> {
    if value.is_empty() {
        return Err(format!("the limit-price {name} is empty"));
    }
    value
        .parse::<u64>()
        .map_err(|error| format!("invalid limit-price {name} `{value}`: {error}"))
}

/// Displays finite fractions as decimals and other fractions as ratios.
fn display_limit_price(value: &LimitPrice) -> String {
    if value.denominator == 0 {
        return format!("{}/{}", value.numerator, value.denominator);
    }
    let divisor = greatest_common_divisor(value.numerator, value.denominator);
    let numerator = value.numerator / divisor;
    let denominator = value.denominator / divisor;
    let mut remainder = denominator;
    let mut twos = 0_u32;
    let mut fives = 0_u32;
    while remainder.is_multiple_of(2) {
        remainder /= 2;
        twos += 1;
    }
    while remainder.is_multiple_of(5) {
        remainder /= 5;
        fives += 1;
    }
    if remainder != 1 {
        return format!("{numerator}/{denominator}");
    }

    let scale = twos.max(fives);
    let Some(power) = 10_u64.checked_pow(scale) else {
        return format!("{numerator}/{denominator}");
    };
    let Some(scaled) = numerator.checked_mul(power / denominator) else {
        return format!("{numerator}/{denominator}");
    };
    if scale == 0 {
        return scaled.to_string();
    }
    let whole = scaled / power;
    let mut fraction = format!("{:0width$}", scaled % power, width = scale as usize);
    while fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{whole}.{fraction}")
}

/// Returns the greatest common divisor of two integers.
fn greatest_common_divisor(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fraction_and_decimal_forms() {
        let fraction = LimitPrice::parse("3/2").unwrap();
        let decimal = LimitPrice::parse("1.5").unwrap();
        assert_eq!(fraction, decimal);
        assert_eq!(fraction.display(), "1.5");
        fraction.validate().unwrap();
    }

    #[test]
    fn rejects_zero_denominator_during_validation() {
        let value = LimitPrice::parse("1/0").unwrap();
        assert!(value.validate().unwrap_err().contains("denominator"));
    }
}
