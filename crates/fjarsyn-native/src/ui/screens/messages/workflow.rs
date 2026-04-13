use super::{MessagesMessage, MessagesScreen};

pub(crate) enum MessagesEffect {
    SendMessage { peer_id: String, body: String },
}

pub(crate) fn execute_messages_message(
    screen: &mut MessagesScreen,
    selected_peer_id: Option<&str>,
    message: MessagesMessage,
) -> Vec<MessagesEffect> {
    match message {
        MessagesMessage::DraftChanged(value) => {
            screen.draft = value;
            Vec::new()
        }
        MessagesMessage::SendPressed => {
            build_send_effect(screen, selected_peer_id).into_iter().collect()
        }
        MessagesMessage::ClearDraft(peer_id) => {
            if selected_peer_id == Some(peer_id.as_str()) {
                screen.draft.clear();
            }
            Vec::new()
        }
    }
}

fn build_send_effect(
    screen: &mut MessagesScreen,
    selected_peer_id: Option<&str>,
) -> Option<MessagesEffect> {
    let peer_id = selected_peer_id?.to_string();
    let body = screen.draft.trim().to_string();

    if body.is_empty() {
        return None;
    }

    Some(MessagesEffect::SendMessage { peer_id, body })
}
