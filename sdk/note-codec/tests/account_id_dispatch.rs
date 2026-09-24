//! Verifies structural codec dispatch for a type that contains a protocol leaf.

use miden_note_codec::{AuthorTypeCodec, export_codecs, from_wit_text, note_codec};
use miden_protocol::account::AccountId;

from_wit_text!(
    r#"
package example:account-codec@1.0.0;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{account-id};

    record account-label {
        account: account-id,
        serial: u64,
    }

    record account-note {
        label: account-label,
    }

    type storage = account-note;
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record account-id { prefix: felt, suffix: felt }
    }
}
"#
);

#[note_codec]
impl AuthorTypeCodec for AccountLabel {
    fn parse(value: &str) -> Result<Self, String> {
        let (account, serial) = value
            .split_once(',')
            .ok_or_else(|| "an account label must use `account-id,serial`".to_owned())?;
        let (account, _) = AccountId::parse(account).map_err(|error| error.to_string())?;
        let serial = serial.parse::<u64>().map_err(|error| error.to_string())?;
        Ok(Self { account, serial })
    }

    fn display(&self) -> String {
        format!("{},{}", self.account.to_hex(), self.serial)
    }

    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

export_codecs!();

#[test]
fn account_id_codec_uses_generated_structural_traits() {
    let account = AccountId::try_from(0xaa00_0000_0000_bc11_0000_bc00_0000_de00u128).unwrap();
    let input = format!("{},7", account.to_hex());
    let fqn = AccountLabel::WIT_FQN;

    let encoded = __miden_note_codec_dispatch::parse(fqn, &input).unwrap();
    assert_eq!(encoded, [account.prefix().as_u64(), account.suffix().as_canonical_u64(), 7, 0]);
    __miden_note_codec_dispatch::validate(fqn, &encoded).unwrap();
    assert_eq!(__miden_note_codec_dispatch::display(fqn, &encoded).unwrap(), input);
}
