use std::sync::Arc;

use super::{ConversationMap, ConversationMessage, ConversationSummary};
use crate::identity::PeerId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conversations {
    pub conversations: Arc<ConversationMap>,
    pub summaries: Arc<Vec<ConversationSummary>>,
}

impl Conversations {
    pub fn messages_for_peer(&self, peer_id: &PeerId) -> Arc<Vec<ConversationMessage>> {
        self.conversations.get(peer_id).cloned().unwrap_or_else(|| Arc::new(Vec::new()))
    }
}

pub(super) fn build(messages: Vec<ConversationMessage>) -> Conversations {
    let mut conversations = std::collections::HashMap::<PeerId, Vec<ConversationMessage>>::new();
    for message in messages {
        conversations.entry(message.peer_id.clone()).or_default().push(message);
    }

    let conversations = conversations
        .into_iter()
        .map(|(peer_id, mut messages)| {
            sort_messages(&mut messages);
            (peer_id, Arc::new(messages))
        })
        .collect::<ConversationMap>();
    let summaries = build_summaries(&conversations);

    Conversations { conversations: Arc::new(conversations), summaries }
}

fn build_summaries(conversations: &ConversationMap) -> Arc<Vec<ConversationSummary>> {
    let mut summaries = conversations
        .values()
        .filter_map(|messages| messages.last().map(ConversationSummary::from))
        .collect::<Vec<_>>();

    summaries.sort_by(|left, right| {
        right
            .last_message_at
            .cmp(&left.last_message_at)
            .then_with(|| right.last_message_row_id.cmp(&left.last_message_row_id))
    });

    Arc::new(summaries)
}

pub(super) fn with_upserted_message(
    conversations_state: &Conversations,
    message: ConversationMessage,
) -> Conversations {
    let peer_id = message.peer_id.clone();
    let mut conversations = (*conversations_state.conversations).clone();
    let mut messages =
        conversations.get(&peer_id).map(|messages| (**messages).clone()).unwrap_or_default();

    if let Some(existing) = messages.iter_mut().find(|existing| existing.id == message.id) {
        *existing = message;
    } else {
        messages.push(message);
    }
    sort_messages(&mut messages);
    conversations.insert(peer_id, Arc::new(messages));
    let summaries = build_summaries(&conversations);

    Conversations { conversations: Arc::new(conversations), summaries }
}

fn sort_messages(messages: &mut [ConversationMessage]) {
    messages.sort_by(|left, right| {
        left.created_at.cmp(&right.created_at).then_with(|| left.id.cmp(&right.id))
    });
}
