use std::sync::{Arc, RwLock};

use super::{MessagingError, MessagingService};
use crate::{
    communication::messaging::{
        ConversationMap, ConversationMessage, ConversationSummary, build_conversation_caches,
        build_conversation_summaries, upsert_conversation_message,
    },
    repositories::MessagesStore,
};

impl MessagingService {
    pub(super) async fn load_messages(
        repository: &Arc<dyn MessagesStore>,
    ) -> Result<Vec<ConversationMessage>, MessagingError> {
        repository
            .list()
            .await
            .map_err(MessagingError::Database)?
            .into_iter()
            .map(|message| ConversationMessage::try_from(message).map_err(MessagingError::from))
            .collect()
    }

    pub(super) fn build_caches(
        messages: Vec<ConversationMessage>,
    ) -> (ConversationMap, Arc<Vec<ConversationSummary>>) {
        build_conversation_caches(messages)
    }

    pub(super) fn build_summaries(
        conversations: &ConversationMap,
    ) -> Arc<Vec<ConversationSummary>> {
        build_conversation_summaries(conversations)
    }

    pub(super) fn cache_message_snapshot(
        conversations: &Arc<RwLock<ConversationMap>>,
        summaries: &Arc<RwLock<Arc<Vec<ConversationSummary>>>>,
        message: ConversationMessage,
    ) {
        let mut conversations_lock = conversations.write().unwrap();
        upsert_conversation_message(&mut conversations_lock, message);

        let new_summaries = Self::build_summaries(&conversations_lock);
        drop(conversations_lock);

        let mut summaries_lock = summaries.write().unwrap();
        *summaries_lock = new_summaries;
    }
}
