macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(uuid::Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4())
            }

            pub fn as_uuid(self) -> uuid::Uuid {
                self.0
            }

            pub const fn from_uuid(value: uuid::Uuid) -> Self {
                Self(value)
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::peer_session::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                uuid::Uuid::parse_str(value).map(Self).map_err(|_| {
                    crate::peer_session::Error::InvalidIdentifier {
                        kind: stringify!($name),
                        value: value.to_owned(),
                    }
                })
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(MessageId);
uuid_id!(SessionId);
uuid_id!(ShareId);
