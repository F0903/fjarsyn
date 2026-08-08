use crate::{peer_session::SessionId, screen_share::LocalShareBinding};

#[derive(Debug, Default)]
pub(in crate::screen_share) struct Plan {
    pub(in crate::screen_share) teardown_pipeline: Option<LocalShareBinding>,
    pub(in crate::screen_share) stop_shares: Vec<LocalShareBinding>,
    pub(in crate::screen_share) confirmed_stop: Option<LocalShareBinding>,
}

impl Plan {
    pub(super) fn reconcile(
        active_shares: &[LocalShareBinding],
        pipeline: Option<(LocalShareBinding, bool)>,
        pending_start_session: Option<SessionId>,
        pending_stop: Option<LocalShareBinding>,
    ) -> Self {
        let mut plan = Self::default();
        if let Some((binding, stop_requested)) = pipeline {
            let exact_active = active_shares.contains(&binding);
            if !exact_active {
                plan.teardown_pipeline = Some(binding);
            } else if stop_requested {
                plan.stop_shares.push(binding);
            }
            plan.stop_shares
                .extend(active_shares.iter().copied().filter(|active| *active != binding));
        } else {
            plan.stop_shares.extend(
                active_shares
                    .iter()
                    .copied()
                    .filter(|binding| pending_start_session != Some(binding.session_id)),
            );
        }
        if let Some(pending_stop) = pending_stop {
            if active_shares.contains(&pending_stop) {
                plan.stop_shares.push(pending_stop);
            } else {
                plan.confirmed_stop = Some(pending_stop);
            }
        }
        plan.stop_shares.sort_unstable();
        plan.stop_shares.dedup();
        plan
    }
}
