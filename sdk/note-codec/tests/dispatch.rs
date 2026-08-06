//! Native tests for generated author codec dispatch.

use miden_note_codec::AuthorTypeCodec;

miden_note_codec::from_wit_text!(
    r#"
package example:codec-schema@1.0.0;

interface note-storage {
    record ratio {
        numerator: u64,
        denominator: u64,
    }

    record codec-note {
        ratio: ratio,
    }

    type storage = codec-note;
}
"#
);

#[miden_note_codec::note_codec]
impl AuthorTypeCodec for Ratio {
    fn parse(value: &str) -> Result<Self, String> {
        let (numerator, denominator) = value
            .split_once('/')
            .ok_or_else(|| "a ratio must use `numerator/denominator`".to_owned())?;
        Ok(Self {
            numerator: numerator.parse::<u64>().map_err(|error| error.to_string())?,
            denominator: denominator.parse::<u64>().map_err(|error| error.to_string())?,
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

miden_note_codec::export_codecs!();

#[test]
fn marked_codec_dispatches_by_canonical_wit_fqn() {
    const RATIO_FQN: &str = "example:codec-schema/note-storage@1.0.0.ratio";

    assert_eq!(__miden_note_codec_dispatch::supported_types(), [RATIO_FQN]);
    let encoded = __miden_note_codec_dispatch::parse(RATIO_FQN, "3/2").unwrap();
    assert_eq!(encoded, [3, 0, 2, 0]);
    assert_eq!(__miden_note_codec_dispatch::display(RATIO_FQN, &encoded).unwrap(), "3/2");
    __miden_note_codec_dispatch::validate(RATIO_FQN, &encoded).unwrap();

    let invalid = __miden_note_codec_dispatch::parse(RATIO_FQN, "3/0").unwrap();
    assert!(
        __miden_note_codec_dispatch::validate(RATIO_FQN, &invalid)
            .unwrap_err()
            .contains("denominator")
    );
    assert!(
        __miden_note_codec_dispatch::parse("example:unknown/type", "3/2")
            .unwrap_err()
            .contains("no note codec is registered")
    );
}
