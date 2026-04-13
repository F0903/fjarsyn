use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use chrono::{Duration, TimeZone};

use super::{
    ConversationMessage, ConversationSummary, MessageDirection, MessageStatus, MessagingService,
};
use crate::{database::MessageModel, repositories::MessagesStore};

#[derive(Default)]
struct FakeMessagesStore {
    messages: Vec<MessageModel>,
}

#[async_trait]
impl MessagesStore for FakeMessagesStore {
    async fn list(&self) -> Result<Vec<MessageModel>, crate::Error> {
        Ok(self.messages.clone())
    }

    async fn get_by_id(&self, id: i64) -> Result<Option<MessageModel>, crate::Error> {
        Ok(self.messages.iter().find(|message| message.id == id).cloned())
    }

    async fn get_by_message_id_and_direction(
        &self,
        message_id: String,
        direction: String,
    ) -> Result<Option<MessageModel>, crate::Error> {
        Ok(self
            .messages
            .iter()
            .find(|message| message.message_id == message_id && message.direction == direction)
            .cloned())
    }

    async fn create_outgoing(
        &self,
        _message_id: String,
        _peer_id: String,
        _body: String,
        _created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, crate::Error> {
        unreachable!("not needed in this test fake")
    }

    async fn create_incoming_if_missing(
        &self,
        _message_id: String,
        _peer_id: String,
        _body: String,
        _created_at: chrono::DateTime<chrono::Utc>,
        _delivered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, crate::Error> {
        unreachable!("not needed in this test fake")
    }

    async fn mark_delivered(
        &self,
        _message_id: String,
        _delivered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, crate::Error> {
        unreachable!("not needed in this test fake")
    }

    async fn mark_failed(&self, _message_id: String) -> Result<bool, crate::Error> {
        unreachable!("not needed in this test fake")
    }
}

fn message(id: i64, peer_id: &str, minutes: i64, body: &str) -> ConversationMessage {
    ConversationMessage {
        id,
        message_id: format!("message-{id}"),
        peer_id: peer_id.to_string(),
        direction: MessageDirection::Outgoing,
        body: body.to_string(),
        status: MessageStatus::Delivered,
        created_at: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
            + Duration::minutes(minutes),
        delivered_at: None,
    }
}

#[test]
fn build_summaries_orders_by_latest_message() {
    let mut conversations = HashMap::new();
    conversations.insert("peer-a".into(), Arc::new(vec![message(1, "peer-a", 1, "older")]));
    conversations.insert("peer-b".into(), Arc::new(vec![message(2, "peer-b", 5, "newer")]));

    let summaries = MessagingService::build_summaries(&conversations);

    assert_eq!(
        summaries.iter().map(|summary| summary.peer_id.as_str()).collect::<Vec<_>>(),
        vec!["peer-b", "peer-a"]
    );
}

#[test]
fn cache_message_updates_one_thread_and_summary() {
    let conversations = Arc::new(RwLock::new(HashMap::from([(
        "peer-a".to_string(),
        Arc::new(vec![message(1, "peer-a", 1, "first")]),
    )])));
    let summaries = Arc::new(RwLock::new(Arc::new(vec![ConversationSummary::from(
        conversations.read().unwrap()["peer-a"].last().unwrap(),
    )])));

    MessagingService::cache_message_snapshot(
        &conversations,
        &summaries,
        message(2, "peer-a", 2, "second"),
    );

    let cached_messages = conversations.read().unwrap()["peer-a"].clone();
    assert_eq!(cached_messages.len(), 2);
    assert_eq!(cached_messages.last().unwrap().body, "second");
    assert_eq!(summaries.read().unwrap().first().unwrap().last_message_body, "second");
}

#[tokio::test]
async fn load_messages_works_with_fake_store() {
    let created_at = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let repository: Arc<dyn MessagesStore> = Arc::new(FakeMessagesStore {
        messages: vec![MessageModel {
            id: 1,
            message_id: "msg-1".into(),
            peer_id: "peer-a".into(),
            direction: "incoming".into(),
            body: "hello".into(),
            status: "delivered".into(),
            created_at,
            delivered_at: Some(created_at),
        }],
    });

    let messages = MessagingService::load_messages(&repository).await.unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].peer_id, "peer-a");
    assert_eq!(messages[0].body, "hello");
    assert_eq!(messages[0].direction, MessageDirection::Incoming);
}
