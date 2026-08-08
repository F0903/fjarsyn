//! Private command routing and session-runtime orchestration.

use std::sync::atomic::{AtomicU64, Ordering};

mod command;
mod runtime;

static NEXT_TRUST_BARRIER_OWNER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TrustBarrierOwnerId(u64);

impl TrustBarrierOwnerId {
    pub(crate) fn allocate() -> Self {
        Self(
            NEXT_TRUST_BARRIER_OWNER_ID
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| next.checked_add(1))
                .expect("trust barrier owner ID space exhausted"),
        )
    }
}

pub(in crate::peer_session::service) use command::Command;
pub(in crate::peer_session::service) use runtime::Runtime;
