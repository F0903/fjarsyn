//! Read-only application projections available to presentation code.

use std::sync::Arc;

use fjarsyn_engine::{
    config::Config,
    contacts::Contact,
    identity::PeerId,
    messaging::{ConversationMap, ConversationMessage, ConversationSummary},
    peer_session::{self, SessionId, SessionSnapshot},
    presence, screen_share,
};

/// Borrowed application projections used to construct a presentation context.
#[derive(Clone, Copy)]
pub(in crate::ui) struct Inputs<'a> {
    pub(in crate::ui) config: &'a Config,
    pub(in crate::ui) local_peer_id: Option<&'a PeerId>,
    pub(in crate::ui) local_public_key: Option<&'a str>,
    pub(in crate::ui) contacts: &'a [Contact],
    pub(in crate::ui) presence: &'a presence::Snapshot,
    pub(in crate::ui) sessions: &'a peer_session::Snapshot,
    pub(in crate::ui) conversation_summaries: &'a [ConversationSummary],
    pub(in crate::ui) conversations: &'a ConversationMap,
    pub(in crate::ui) screen_share: &'a screen_share::Snapshot,
}

/// A deliberately narrow, immutable view of the projections rendered by the UI.
#[derive(Clone, Copy)]
pub(in crate::ui) struct Context<'a> {
    inputs: Inputs<'a>,
}

impl<'a> Context<'a> {
    pub(in crate::ui) fn new(inputs: Inputs<'a>) -> Self {
        Self { inputs }
    }

    pub(in crate::ui) fn config(self) -> &'a Config {
        self.inputs.config
    }

    pub(in crate::ui) fn local_peer_id(self) -> Option<&'a PeerId> {
        self.inputs.local_peer_id
    }

    pub(in crate::ui) fn local_identity(self) -> Option<(&'a PeerId, &'a str)> {
        self.inputs.local_peer_id.zip(self.inputs.local_public_key)
    }

    pub(in crate::ui) fn contacts(self) -> &'a [Contact] {
        self.inputs.contacts
    }

    pub(in crate::ui) fn display_name(self, peer_id: &PeerId) -> String {
        self.inputs
            .contacts
            .iter()
            .find(|contact| &contact.peer_id == peer_id)
            .map(|contact| contact.name.clone())
            .unwrap_or_else(|| peer_id.to_string())
    }

    pub(in crate::ui) fn is_nearby(self, peer_id: &PeerId) -> bool {
        self.inputs.presence.is_nearby(peer_id)
    }

    pub(in crate::ui) fn session_for_peer(self, peer_id: &PeerId) -> Option<&'a SessionSnapshot> {
        self.inputs.sessions.session_for_peer(peer_id)
    }

    pub(in crate::ui) fn connected_session_id(self, peer_id: &PeerId) -> Option<SessionId> {
        self.session_for_peer(peer_id)
            .filter(|session| session.phase == peer_session::Phase::Connected)
            .map(|session| session.session_id)
    }

    pub(in crate::ui) fn conversation_summaries(self) -> &'a [ConversationSummary] {
        self.inputs.conversation_summaries
    }

    pub(in crate::ui) fn messages_for_peer(
        self,
        peer_id: &PeerId,
    ) -> Arc<Vec<ConversationMessage>> {
        self.inputs.conversations.get(peer_id).cloned().unwrap_or_else(|| Arc::new(Vec::new()))
    }

    pub(in crate::ui) fn screen_share_session(
        self,
        session_id: SessionId,
    ) -> screen_share::SessionSnapshot {
        self.inputs.screen_share.session(session_id)
    }

    pub(in crate::ui) fn encoder_restart_required(self) -> bool {
        self.inputs.screen_share.encoder_restart_required()
    }

    pub(in crate::ui) fn decoder_restart_required(self) -> bool {
        self.inputs.screen_share.decoder_restart_required()
    }
}
