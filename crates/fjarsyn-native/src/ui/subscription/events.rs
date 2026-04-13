use fjarsyn_core::{
    networking::discovery::DiscoveryEvent,
    services::{call_service::CallEvent, messaging_service::MessagingEvent},
};
use iced::Subscription;

use super::receiver::{EventReceiverRef, channel_subscription};
use crate::ui::message::{CallServiceMessage, Message, MessagingServiceMessage};

pub(super) fn call_event_subscription(
    receiver: EventReceiverRef<CallEvent>,
) -> Subscription<Message> {
    channel_subscription(receiver, map_call_event)
}

pub(super) fn discovery_event_subscription(
    receiver: EventReceiverRef<DiscoveryEvent>,
) -> Subscription<Message> {
    channel_subscription(receiver, map_discovery_event)
}

pub(super) fn messaging_event_subscription(
    receiver: EventReceiverRef<MessagingEvent>,
) -> Subscription<Message> {
    channel_subscription(receiver, map_messaging_event)
}

fn map_call_event(event: CallEvent) -> Message {
    Message::CallService(CallServiceMessage::CallEvent(event))
}

fn map_discovery_event(event: DiscoveryEvent) -> Message {
    Message::CallService(CallServiceMessage::DiscoveryEvent(event))
}

fn map_messaging_event(event: MessagingEvent) -> Message {
    Message::Messaging(MessagingServiceMessage::Event(event))
}
