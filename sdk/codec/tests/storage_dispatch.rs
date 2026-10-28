//! Native tests for codec dispatch over the custom types of an account storage schema.

use miden_codec::AuthorTypeCodec;

miden_codec::from_wit_text!(
    r#"
package example:codec-storage-schema;

interface account-storage {
    record ratio {
        numerator: u32,
        denominator: u32,
    }

    record pair {
        left: u32,
        right: u32,
    }

    record storage {
        price: ratio,
        prices: list<tuple<pair, ratio>>,
    }
}
"#
);

#[miden_codec::codec]
impl AuthorTypeCodec for Ratio {
    fn parse(value: &str) -> Result<Self, String> {
        let (numerator, denominator) = value
            .split_once('/')
            .ok_or_else(|| "a ratio must use `numerator/denominator`".to_owned())?;
        Ok(Self {
            numerator: numerator.parse::<u32>().map_err(|error| error.to_string())?,
            denominator: denominator.parse::<u32>().map_err(|error| error.to_string())?,
        })
    }

    fn display(&self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }

    fn validate(&self) -> Result<(), String> {
        if self.denominator == 0 {
            Err("the denominator must not be zero".to_owned())
        } else {
            Ok(())
        }
    }
}

#[miden_codec::codec]
impl AuthorTypeCodec for Pair {
    fn parse(value: &str) -> Result<Self, String> {
        let (left, right) =
            value.split_once(',').ok_or_else(|| "a pair must use `left,right`".to_owned())?;
        Ok(Self {
            left: left.trim().parse::<u32>().map_err(|error| error.to_string())?,
            right: right.trim().parse::<u32>().map_err(|error| error.to_string())?,
        })
    }

    fn display(&self) -> String {
        format!("{},{}", self.left, self.right)
    }

    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

miden_codec::export_codecs!();

#[test]
fn slot_value_and_map_key_types_dispatch_by_canonical_wit_fqn() {
    const PAIR_FQN: &str = "example:codec-storage-schema/account-storage.pair";
    const RATIO_FQN: &str = "example:codec-storage-schema/account-storage.ratio";

    assert_eq!(__miden_codec_dispatch::supported_types(), [PAIR_FQN, RATIO_FQN]);
    let ratio = __miden_codec_dispatch::parse(RATIO_FQN, "3/2").unwrap();
    assert_eq!(ratio, [3, 2]);
    assert_eq!(__miden_codec_dispatch::display(RATIO_FQN, &ratio).unwrap(), "3/2");
    let invalid = __miden_codec_dispatch::parse(RATIO_FQN, "3/0").unwrap();
    assert!(
        __miden_codec_dispatch::validate(RATIO_FQN, &invalid)
            .unwrap_err()
            .contains("denominator")
    );

    let pair = __miden_codec_dispatch::parse(PAIR_FQN, "4, 5").unwrap();
    assert_eq!(pair, [4, 5]);
    assert_eq!(__miden_codec_dispatch::display(PAIR_FQN, &pair).unwrap(), "4,5");
}
