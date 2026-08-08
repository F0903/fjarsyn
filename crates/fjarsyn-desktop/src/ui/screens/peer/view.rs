use std::sync::Arc;

use fjarsyn_engine::{
    media::{frame::Frame, gpu_interop},
    messaging::{ConversationMessage, MessageDirection, MessageStatus},
    peer_session::{LocalShareState, Phase, RemoteShareState, SessionId, SessionSnapshot},
    screen_share,
};
use iced::{
    Alignment, Border, Color, Element, Length,
    widget::{Space, button, column, container, row, scrollable, stack, text, text_input},
};
use iced_fonts::lucide;

use super::Screen;
use crate::ui::{
    components::{CpuFrameViewer, GpuFrameViewer},
    fonts,
    message::{self, Message, screen::PeerMessage},
    presentation::{Context, project_peer},
    theme,
};

impl Screen {
    pub(super) fn render_view<'a>(&'a self, context: Context<'a>) -> Element<'a, Message> {
        let session = context.session_for_peer(&self.peer_id);
        let session_id = session.map(|session| session.session_id);
        let share = session_id.map(|id| context.screen_share_session(id)).unwrap_or_default();

        let header = self.view_header(context, session);
        let body = row![
            self.view_video(context, session, &share).width(Length::FillPortion(3)),
            self.view_messages(context, session).width(Length::FillPortion(2)),
        ]
        .spacing(16)
        .height(Length::Fill);

        container(column![header, body].spacing(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }

    fn view_header<'a>(
        &'a self,
        context: Context<'a>,
        session: Option<&SessionSnapshot>,
    ) -> Element<'a, Message> {
        let name = context.display_name(&self.peer_id);
        let nearby = context.is_nearby(&self.peer_id);
        let phase = session.map(|session| session.phase);
        let presentation = project_peer(nearby, phase);
        let session_label = phase.map(session_phase_label).unwrap_or("Disconnected");

        let action: Element<'_, Message> = match (session, phase) {
            (None, _) => {
                let mut connect =
                    button(row![lucide::unplug().size(15), text("Connect").size(13)].spacing(8))
                        .padding([9, 13])
                        .style(|theme, status| theme::button_style(theme, status, true));
                if presentation.can_connect() {
                    connect = connect.on_press(Message::PeerAction(
                        message::peer::Action::Connect(self.peer_id.clone()),
                    ));
                }
                connect.into()
            }
            (Some(session), Some(Phase::Incoming)) => row![
                button(row![lucide::check().size(14), text("Accept")].spacing(7))
                    .on_press(Message::PeerAction(message::peer::Action::Accept {
                        session_id: session.session_id,
                    }))
                    .padding([9, 12])
                    .style(button::success),
                button(row![lucide::x().size(14), text("Reject")].spacing(7))
                    .on_press(Message::PeerAction(message::peer::Action::Reject {
                        session_id: session.session_id,
                    }))
                    .padding([9, 12])
                    .style(button::danger),
            ]
            .spacing(8)
            .into(),
            (Some(session), _) => {
                let mut disconnect =
                    button(row![lucide::unplug().size(14), text("Disconnect")].spacing(7))
                        .padding([9, 12])
                        .style(theme::danger_button_style);
                if presentation.can_disconnect() {
                    disconnect = disconnect.on_press(Message::PeerAction(
                        message::peer::Action::Disconnect { session_id: session.session_id },
                    ));
                }
                disconnect.into()
            }
        };

        row![
            column![
                text(name).size(28).font(fonts::BOLD).style(text::primary),
                text(self.peer_id.to_string()).size(11).style(text::secondary),
            ]
            .spacing(3)
            .width(Length::Fill),
            column![
                text(format!("Presence: {}", if nearby { "Nearby" } else { "Away" }))
                    .size(11)
                    .style(text::secondary),
                text(format!("Session: {session_label}")).size(11).style(text::secondary),
            ]
            .spacing(3)
            .align_x(Alignment::End),
            action,
        ]
        .spacing(18)
        .align_y(Alignment::Center)
        .into()
    }

    fn view_video<'a>(
        &'a self,
        context: Context<'a>,
        session: Option<&SessionSnapshot>,
        share: &screen_share::SessionSnapshot,
    ) -> iced::widget::Container<'a, Message> {
        let connected =
            project_peer(false, session.map(|session| session.phase)).capabilities_ready();
        let reconnecting = session.is_some_and(|session| session.phase == Phase::Reconnecting);
        let remote_binding = session.and_then(|session| match session.remote_share {
            RemoteShareState::Active { share_id, epoch } => {
                Some(screen_share::ShareBinding::new(share_id, epoch))
            }
            RemoteShareState::Inactive => None,
        });
        let local_binding = session.and_then(|session| match session.local_share {
            LocalShareState::Active { share_id, epoch } => {
                Some(screen_share::ShareBinding::new(share_id, epoch))
            }
            LocalShareState::Inactive => None,
        });
        let remote_active = remote_binding.is_some();
        let session_id = session.map(|session| session.session_id);

        let remote_failure = match &share.remote {
            screen_share::RemoteState::Failed(reason) => Some(reason.clone()),
            _ => None,
        };
        let decoder_restart_required = context.decoder_restart_required();
        let authenticated_frame = share.remote_frame.clone().filter(|_| {
            should_render_remote_frame(
                decoder_restart_required,
                remote_binding,
                share.remote_frame_binding,
            )
        });
        let remote: Element<'_, Message> = if decoder_restart_required {
            container(
                column![
                    lucide::triangle_alert().size(28),
                    text("Video unavailable").size(18),
                    text("Restart Fjarsyn to view shared screens.").size(12).style(text::secondary),
                ]
                .spacing(10)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into()
        } else if let Some(frame) = authenticated_frame {
            self.frame_viewer(frame)
        } else {
            container(
                column![
                    if remote_active {
                        lucide::video().size(28)
                    } else {
                        lucide::monitor().size(28)
                    },
                    text(if remote_failure.is_some() {
                        "Remote video failed"
                    } else if reconnecting {
                        "Reconnecting"
                    } else if !connected {
                        "Not connected"
                    } else if remote_active {
                        "Receiving shared screen"
                    } else {
                        "Waiting for screen share"
                    })
                    .size(18),
                    text(if let Some(reason) = remote_failure {
                        reason
                    } else if reconnecting {
                        "Keeping the current screen share ready while the connection recovers."
                            .into()
                    } else if connected {
                        "Messaging remains available while no screen is being shared.".into()
                    } else {
                        "Connect to enable messaging and screen sharing.".into()
                    })
                    .size(12)
                    .style(text::secondary),
                ]
                .spacing(10)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into()
        };

        let preview_enabled = context.config().capture.enable_ui_preview;
        let encoder_restart_required = context.encoder_restart_required();
        let local_preview: Element<'_, Message> =
            if !encoder_restart_required && preview_enabled && self.local_preview_visible {
                if let Some(frame) = share.local_frame.clone().filter(|_| {
                    local_binding.is_some() && share.local_frame_binding == local_binding
                }) {
                    container(self.frame_viewer(frame))
                        .width(Length::Fixed(260.0))
                        .height(Length::Fixed(146.0))
                        .padding(6)
                        .style(theme::card_container)
                        .into()
                } else {
                    Space::new().into()
                }
            } else {
                Space::new().into()
            };

        let core_local_active = local_binding.is_some();
        let share_controls = self.view_share_controls(
            session_id,
            connected,
            core_local_active,
            preview_enabled,
            encoder_restart_required,
            share,
        );
        container(stack![
            container(remote).width(Length::Fill).height(Length::Fill),
            container(local_preview)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::Start)
                .padding(16),
            container(share_controls)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::End)
                .padding(16),
        ])
        .style(theme::card_container)
    }

    fn view_share_controls(
        &self,
        session_id: Option<SessionId>,
        connected: bool,
        core_local_active: bool,
        preview_enabled: bool,
        encoder_restart_required: bool,
        share: &screen_share::SessionSnapshot,
    ) -> Element<'_, Message> {
        let local_active = matches!(share.local, screen_share::LocalState::Active);
        let reconciling_stop = core_local_active
            && matches!(
                share.local,
                screen_share::LocalState::Inactive | screen_share::LocalState::Failed(_)
            );
        let busy = encoder_restart_required
            || reconciling_stop
            || matches!(
                share.local,
                screen_share::LocalState::Selecting
                    | screen_share::LocalState::Starting
                    | screen_share::LocalState::Stopping
            );
        let label = if encoder_restart_required && !reconciling_stop {
            "Sharing unavailable"
        } else if reconciling_stop {
            "Stopping..."
        } else {
            match &share.local {
                screen_share::LocalState::Inactive | screen_share::LocalState::Failed(_) => {
                    "Share screen"
                }
                screen_share::LocalState::Selecting => "Selecting...",
                screen_share::LocalState::Starting => "Starting...",
                screen_share::LocalState::Active => "Stop sharing",
                screen_share::LocalState::Stopping => "Stopping...",
            }
        };

        let mut share_button = button(
            row![
                if local_active {
                    lucide::screen_share_off().size(15)
                } else {
                    lucide::screen_share().size(15)
                },
                text(label).size(13),
            ]
            .spacing(8),
        )
        .padding([9, 13])
        .style(move |theme, status| {
            if local_active {
                theme::danger_button_style(theme, status)
            } else {
                theme::button_style(theme, status, true)
            }
        });

        if connected
            && !busy
            && let Some(session_id) = session_id
        {
            let action = if local_active {
                message::peer::Action::StopScreenShare { session_id }
            } else {
                message::peer::Action::BeginScreenShare { session_id }
            };
            share_button = share_button.on_press(Message::PeerAction(action));
        }

        let mut controls = row![share_button].spacing(8).align_y(Alignment::Center);
        if local_active && preview_enabled {
            controls = controls.push(
                button(if self.local_preview_visible {
                    lucide::panel_top_close().size(15)
                } else {
                    lucide::panel_top_open().size(15)
                })
                .on_press(Message::Screen(message::Screen::Peer(PeerMessage::ToggleLocalPreview)))
                .padding(9)
                .style(|theme, status| theme::button_style(theme, status, false)),
            );
        }
        let mut content = column![controls].spacing(6).align_x(Alignment::Center);
        if encoder_restart_required {
            content = content
                .push(text("Restart Fjarsyn to share screens.").size(11).style(text::secondary));
        } else if let screen_share::LocalState::Failed(reason) = &share.local {
            content = content.push(text(reason.clone()).size(11).style(text::secondary));
        }
        container(content).padding(8).style(theme::section_container).into()
    }

    fn view_messages<'a>(
        &'a self,
        context: Context<'a>,
        session: Option<&SessionSnapshot>,
    ) -> iced::widget::Container<'a, Message> {
        let messages = context.messages_for_peer(&self.peer_id);
        let mut transcript = column![].spacing(10);
        if messages.is_empty() {
            transcript = transcript.push(
                container(text("No messages yet.").size(13).style(text::secondary))
                    .padding(14)
                    .width(Length::Fill),
            );
        } else {
            for message in messages.iter() {
                transcript = transcript.push(self.message_bubble(message.clone()));
            }
        }

        let connected =
            project_peer(false, session.map(|session| session.phase)).capabilities_ready();
        let reconnecting = session.is_some_and(|session| session.phase == Phase::Reconnecting);
        let mut input = text_input(
            if connected {
                "Type a message..."
            } else if reconnecting {
                "Reconnecting..."
            } else {
                "Connect to send messages"
            },
            &self.draft,
        )
        .padding(11)
        .style(theme::text_input_style)
        .width(Length::Fill);
        if connected {
            input = input
                .on_input(|value| {
                    Message::Screen(message::Screen::Peer(PeerMessage::DraftChanged(value)))
                })
                .on_submit(Message::Screen(message::Screen::Peer(PeerMessage::SendPressed)));
        }
        let mut send = button(lucide::send().size(15))
            .padding(11)
            .style(|theme, status| theme::button_style(theme, status, true));
        if connected && !self.draft.trim().is_empty() {
            send = send.on_press(Message::Screen(message::Screen::Peer(PeerMessage::SendPressed)));
        }

        container(
            column![
                row![lucide::message_square().size(16), text("Messages").size(16)]
                    .spacing(8)
                    .align_y(Alignment::Center),
                container(scrollable(transcript)).height(Length::Fill),
                row![input, send].spacing(8).align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .padding(14)
        .style(theme::card_container)
    }

    fn message_bubble(&self, message: ConversationMessage) -> Element<'static, Message> {
        let outgoing = message.direction == MessageDirection::Outgoing;
        let status = match message.status {
            MessageStatus::Pending => "Pending",
            MessageStatus::Sent => "Sent",
            MessageStatus::Delivered => "Delivered",
            MessageStatus::Unknown => "Delivery uncertain",
            MessageStatus::Failed => "Failed",
        };
        let bubble = container(
            column![
                text(message.body.clone()).size(13),
                text(format!("{} | {status}", message.created_at.format("%H:%M")))
                    .size(10)
                    .style(text::secondary),
            ]
            .spacing(5),
        )
        .padding([8, 10])
        .max_width(340)
        .style(move |_| container::Style {
            background: Some(if outgoing {
                Color { a: 0.18, ..theme::PRIMARY_COLOR }.into()
            } else {
                theme::CARD_BACKGROUND.into()
            }),
            border: Border { color: theme::BORDER_COLOR, width: 1.0, radius: 9.0.into() },
            ..Default::default()
        });
        if outgoing {
            row![Space::new().width(Length::Fill), bubble].into()
        } else {
            row![bubble, Space::new().width(Length::Fill)].into()
        }
    }

    fn frame_viewer(&self, frame: Arc<Frame>) -> Element<'static, Message> {
        if frame.gpu_import_handle().is_some()
            && gpu_interop::supports_zero_copy_preview(frame.format)
        {
            GpuFrameViewer::new(frame).into()
        } else if frame.format.supports_software_preview() {
            CpuFrameViewer::new(frame).into()
        } else {
            container(text("Preview unavailable for this pixel format.").size(12))
                .center(Length::Fill)
                .into()
        }
    }
}

fn session_phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Requesting => "Requesting",
        Phase::Incoming => "Incoming",
        Phase::Negotiating => "Negotiating",
        Phase::Connected => "Connected",
        Phase::Reconnecting => "Reconnecting",
        Phase::Disconnecting => "Disconnecting",
    }
}

fn should_render_remote_frame(
    decoder_restart_required: bool,
    active_binding: Option<screen_share::ShareBinding>,
    frame_binding: Option<screen_share::ShareBinding>,
) -> bool {
    !decoder_restart_required && active_binding.is_some() && active_binding == frame_binding
}

#[cfg(test)]
mod tests {
    use fjarsyn_engine::{
        peer_session::{ShareEpoch, ShareId},
        screen_share::ShareBinding,
    };

    use super::should_render_remote_frame;

    #[test]
    fn remote_frame_requires_authenticated_active_share_state() {
        let active = ShareBinding::new(ShareId::new(), ShareEpoch::FIRST);
        let stale_id = ShareBinding::new(ShareId::new(), active.epoch());
        let stale_epoch = ShareBinding::new(
            active.share_id(),
            ShareEpoch::try_from(active.epoch().value() + 1).unwrap(),
        );

        assert!(!should_render_remote_frame(false, None, Some(active)));
        assert!(!should_render_remote_frame(false, Some(active), None));
        assert!(!should_render_remote_frame(false, Some(active), Some(stale_id)));
        assert!(!should_render_remote_frame(false, Some(active), Some(stale_epoch)));
        assert!(should_render_remote_frame(false, Some(active), Some(active)));
        assert!(!should_render_remote_frame(true, Some(active), Some(active)));
    }
}
