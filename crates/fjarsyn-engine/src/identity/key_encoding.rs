use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ed25519_dalek::VerifyingKey;

use super::Error;

pub(super) const PUBLIC_KEY_BYTES: usize = 32;
pub(super) const PRIVATE_KEY_BYTES: usize = 32;

pub(super) fn decode_verifying_key(public_key: &str) -> Result<VerifyingKey, Error> {
    let bytes = decode_array::<PUBLIC_KEY_BYTES>("public key", public_key)?;
    let key = VerifyingKey::from_bytes(&bytes).map_err(Error::InvalidPublicKey)?;
    if key.is_weak() {
        return Err(Error::WeakPublicKey);
    }
    Ok(key)
}

pub(super) fn decode_array<const N: usize>(
    kind: &'static str,
    value: &str,
) -> Result<[u8; N], Error> {
    let bytes = BASE64.decode(value).map_err(|source| Error::InvalidBase64 { kind, source })?;
    let actual = bytes.len();
    bytes.try_into().map_err(|_| Error::InvalidLength { kind, expected: N, actual })
}
