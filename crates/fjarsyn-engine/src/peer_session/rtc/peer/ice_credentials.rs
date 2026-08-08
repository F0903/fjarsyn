use sha2::{Digest, Sha256};

use crate::peer_session::Error;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct IceCredentials {
    username_fragment: String,
    password_digest: [u8; 32],
}

impl IceCredentials {
    pub(super) fn from_sdp(sdp: &str) -> Result<Self, Error> {
        let mut username_fragment: Option<&str> = None;
        let mut password: Option<&str> = None;
        for line in sdp.lines().map(str::trim) {
            if let Some(value) = line.strip_prefix("a=ice-ufrag:") {
                record_unique_attribute(&mut username_fragment, value, "username fragment")?;
            } else if let Some(value) = line.strip_prefix("a=ice-pwd:") {
                record_unique_attribute(&mut password, value, "password")?;
            }
        }
        let username_fragment = username_fragment
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::Protocol("SDP has no ICE username fragment".into()))?;
        let password = password
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::Protocol("SDP has no ICE password".into()))?;
        Ok(Self {
            username_fragment: username_fragment.to_owned(),
            password_digest: Sha256::digest(password.as_bytes()).into(),
        })
    }

    pub(super) fn username_fragment(&self) -> &str {
        &self.username_fragment
    }

    pub(super) fn require_rotation_from(&self, previous: &Self, side: &str) -> Result<(), Error> {
        if self.username_fragment == previous.username_fragment
            || self.password_digest == previous.password_digest
        {
            return Err(Error::Protocol(format!(
                "ICE restart did not rotate both {side} credentials"
            )));
        }
        Ok(())
    }
}

fn record_unique_attribute<'a>(
    slot: &mut Option<&'a str>,
    value: &'a str,
    name: &str,
) -> Result<(), Error> {
    if let Some(existing) = slot
        && *existing != value
    {
        return Err(Error::Protocol(format!("SDP contains multiple ICE {name}s")));
    }
    *slot = Some(value);
    Ok(())
}
