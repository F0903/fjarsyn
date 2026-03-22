use super::{MessagesMessage, MessagesScreen};

pub(crate) enum MessagesEffect {
    SendMessage { peer_id: String, body: String },
}

pub(crate) fn reduce(screen: &mut MessagesScreen, message: MessagesMessage) -> Vec<MessagesEffect> {
    match message {
        MessagesMessage::DraftChanged(value) => {
            screen.draft = value;
            Vec::new()
        }
        MessagesMessage::SendPressed => build_send_effect(screen).into_iter().collect(),
        MessagesMessage::ClearDraft(peer_id) => {
            if screen.selected_peer_id.as_deref() == Some(peer_id.as_str()) {
                screen.draft.clear();
            }
            Vec::new()
        }
    }
}

fn build_send_effect(screen: &mut MessagesScreen) -> Option<MessagesEffect> {
    let peer_id = screen.selected_peer_id.clone()?;
    let body = screen.draft.trim().to_string();

    if body.is_empty() {
        return None;
    }

    Some(MessagesEffect::SendMessage { peer_id, body })
}
