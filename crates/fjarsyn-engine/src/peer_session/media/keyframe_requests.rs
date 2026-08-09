use std::sync::{Arc, Mutex};

use crate::peer_session::{ShareEpoch, ShareId};

type Binding = (ShareId, ShareEpoch);

#[derive(Debug, Default)]
struct State {
    active: Option<Binding>,
    pending: bool,
}

/// Coalesced force-keyframe requests bound to the active local share.
///
/// Binding and consumption share one lock so a stale sink cannot race a share
/// replacement and consume the replacement share's recovery request.
#[derive(Debug, Clone, Default)]
pub(in crate::peer_session) struct KeyframeRequests {
    state: Arc<Mutex<State>>,
}

impl KeyframeRequests {
    pub(in crate::peer_session) fn activate(&self, share_id: ShareId, epoch: ShareEpoch) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.active = Some((share_id, epoch));
        state.pending = true;
    }

    pub(in crate::peer_session) fn deactivate(&self, share_id: ShareId, epoch: ShareEpoch) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active == Some((share_id, epoch)) {
            state.active = None;
            state.pending = false;
        }
    }

    pub(in crate::peer_session) fn request(&self) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active.is_some() {
            state.pending = true;
        }
    }

    pub(in crate::peer_session) fn take(&self, share_id: ShareId, epoch: ShareEpoch) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active == Some((share_id, epoch)) && state.pending {
            state.pending = false;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_coalesce_and_are_consumed_once() {
        let requests = KeyframeRequests::default();
        let share = ShareId::new();
        requests.activate(share, ShareEpoch::FIRST);
        requests.request();
        requests.request();

        assert!(requests.take(share, ShareEpoch::FIRST));
        assert!(!requests.take(share, ShareEpoch::FIRST));
    }

    #[test]
    fn a_stale_share_cannot_consume_the_replacement_request() {
        let requests = KeyframeRequests::default();
        let stale = ShareId::new();
        let active = ShareId::new();
        requests.activate(stale, ShareEpoch::FIRST);
        requests.activate(active, ShareEpoch::FIRST.next().unwrap());

        assert!(!requests.take(stale, ShareEpoch::FIRST));
        assert!(requests.take(active, ShareEpoch::FIRST.next().unwrap()));
    }

    #[test]
    fn requests_without_an_active_share_do_not_leak_into_the_next_share() {
        let requests = KeyframeRequests::default();
        requests.request();
        let share = ShareId::new();
        requests.activate(share, ShareEpoch::FIRST);

        assert!(requests.take(share, ShareEpoch::FIRST));
        assert!(!requests.take(share, ShareEpoch::FIRST));
    }
}
