use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

const MAX_PEER_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerIdError {
    #[error("peer ID cannot be empty")]
    Empty,
    #[error("peer ID exceeds the {max} byte limit")]
    TooLong { max: usize },
    #[error("peer ID must start with an ASCII letter or digit, got {character:?}")]
    InvalidStart { character: char },
    #[error(
        "peer ID contains invalid character {character:?} at byte {index}; only ASCII letters, digits, '.', '_', and '-' are allowed"
    )]
    InvalidCharacter { index: usize, character: char },
}

/// Stable identifier for a cryptographically trusted peer.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PeerId(String);

impl PeerId {
    pub fn new(value: impl Into<String>) -> Result<Self, PeerIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PeerIdError::Empty);
        }
        if value.len() > MAX_PEER_ID_BYTES {
            return Err(PeerIdError::TooLong { max: MAX_PEER_ID_BYTES });
        }

        let mut characters = value.char_indices();
        let (_, first) = characters.next().expect("non-empty peer ID has a first character");
        if !first.is_ascii_alphanumeric() {
            return Err(PeerIdError::InvalidStart { character: first });
        }
        if let Some((index, character)) =
            characters.find(|(_, character)| !is_peer_id_tail_character(*character))
        {
            return Err(PeerIdError::InvalidCharacter { index, character });
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_peer_id_tail_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}

impl fmt::Debug for PeerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("PeerId").field(&self.0).finish()
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for PeerId {
    type Error = PeerIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for PeerId {
    type Error = PeerIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for PeerId {
    type Err = PeerIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for PeerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_ids_enforce_the_canonical_ascii_grammar_without_trimming() {
        assert_eq!(PeerId::new("peer-A_1.example").unwrap().as_str(), "peer-A_1.example");
        assert_eq!(PeerId::new("").unwrap_err(), PeerIdError::Empty);
        assert_eq!(
            PeerId::new(" peer-a").unwrap_err(),
            PeerIdError::InvalidStart { character: ' ' }
        );
        assert_eq!(
            PeerId::new("peer-a ").unwrap_err(),
            PeerIdError::InvalidCharacter { index: 6, character: ' ' }
        );
        assert_eq!(
            PeerId::new("peer-\u{00e9}").unwrap_err(),
            PeerIdError::InvalidCharacter { index: 5, character: '\u{00e9}' }
        );
        assert_eq!(PeerId::new("-peer").unwrap_err(), PeerIdError::InvalidStart { character: '-' });
    }

    #[test]
    fn peer_id_length_limit_is_inclusive_and_measured_in_bytes() {
        assert!(PeerId::new("a".repeat(MAX_PEER_ID_BYTES)).is_ok());
        assert_eq!(
            PeerId::new("a".repeat(MAX_PEER_ID_BYTES + 1)).unwrap_err(),
            PeerIdError::TooLong { max: MAX_PEER_ID_BYTES }
        );
    }

    #[test]
    fn serde_and_from_str_use_the_same_strict_peer_id_boundary() {
        assert_eq!("peer-a".parse::<PeerId>().unwrap(), PeerId::new("peer-a").unwrap());
        assert!(" peer-a".parse::<PeerId>().is_err());
        assert!(serde_json::from_str::<PeerId>(r#""peer-a ""#).is_err());
    }
}
