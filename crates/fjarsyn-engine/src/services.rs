use std::fmt;

use crate::{
    contacts::ContactsService, media::codec, messaging, peer_session, presence, screen_share,
};

macro_rules! define_services {
    (
        $(#[$attribute:meta])*
        $visibility:vis struct $name:ident {
            direct {
                $($direct_service:ident: $direct_interface:ty),* $(,)?
            }
            hosted {
                $($hosted_service:ident: $hosted_interface:ty),* $(,)?
            }
        }
    ) => {
        $(#[$attribute])*
        $visibility struct $name {
            $($direct_service: $direct_interface,)*
            $($hosted_service: $hosted_interface),*
        }

        impl $name {
            pub(crate) fn new(
                $($direct_service: $direct_interface,)*
                $($hosted_service: $hosted_interface),*
            ) -> Self {
                Self {
                    $($direct_service,)*
                    $($hosted_service),*
                }
            }

            $(
                pub fn $direct_service(&self) -> &$direct_interface {
                    &self.$direct_service
                }
            )*

            $(
                pub fn $hosted_service(&self) -> &$hosted_interface {
                    &self.$hosted_service
                }
            )*
        }
    };
}

define_services! {
    /// Typed interfaces to the capabilities provided by a running [`crate::Engine`].
    ///
    /// This is a passive facade. Service construction, implementation ownership,
    /// and lifecycle coordination remain the responsibility of [`crate::Engine`].
    pub struct Services {
        direct {
            contacts: ContactsService,
        }
        hosted {
            sessions: peer_session::ServiceHandle,
            presence: presence::ServiceHandle,
            messaging: messaging::ServiceHandle,
            codecs: codec::ServiceHandle,
            screen_share: screen_share::ServiceHandle,
        }
    }
}

impl fmt::Debug for Services {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Services").finish_non_exhaustive()
    }
}
