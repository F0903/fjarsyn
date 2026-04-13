use fjarsyn_core::utils::text::truncate;
use iced::{
    Alignment, Element, Length,
    widget::{column, container, text},
};
use iced_fonts::lucide;

use super::{CallScreen, WaitingStateCopy};
use crate::ui::{self, fonts, message::Message, shell::ShellContext};

impl CallScreen {
    pub(super) fn view_remote_video(&self, ctx: ShellContext<'_>) -> Element<'_, Message> {
        match self.remote.latest_frame.clone() {
            Some(frame) => {
                let viewer = self
                    .preferred_frame_viewer(frame)
                    .unwrap_or_else(|| self.view_preview_unavailable());

                container(viewer)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill)
                    .into()
            }
            None => self.view_waiting_state(ctx),
        }
    }

    fn view_waiting_state(&self, ctx: ShellContext<'_>) -> Element<'_, Message> {
        let remote_name = self.remote_name(ctx);
        let waiting_state = self.waiting_state_copy(ctx);

        container(
            container(
                column![
                    container(container(waiting_state.icon.size(26).center()).center(Length::Fill))
                        .width(Length::Fixed(56.0))
                        .height(Length::Fixed(56.0))
                        .padding(8)
                        .style(ui::theme::icon_bubble_container),
                    text(waiting_state.title)
                        .size(34)
                        .font(fonts::outfit::BOLD)
                        .style(text::primary),
                    text(remote_name).size(18).font(fonts::outfit::MEDIUM).style(text::secondary),
                    text(waiting_state.subtitle)
                        .size(13)
                        .font(fonts::outfit::MEDIUM)
                        .style(text::secondary),
                ]
                .spacing(14)
                .align_x(Alignment::Center),
            )
            .padding(28)
            .style(ui::theme::card_container),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .into()
    }

    fn waiting_state_copy(&self, ctx: ShellContext<'_>) -> WaitingStateCopy {
        if ctx.session.call_connected {
            WaitingStateCopy {
                title: "Connected",
                subtitle: "Waiting for video",
                icon: lucide::video(),
            }
        } else {
            WaitingStateCopy {
                title: "Calling...",
                subtitle: "Waiting for answer",
                icon: lucide::phone_outgoing(),
            }
        }
    }

    fn remote_name(&self, ctx: ShellContext<'_>) -> String {
        let peer_id = ctx.session.target_id.as_deref().or(ctx.session.incoming_call_id.as_deref());

        if let Some(peer_id) = peer_id
            && let Some(peer) =
                ctx.networking.discovered_peers.iter().find(|peer| peer.id == peer_id)
            && !peer.instance_name.trim().is_empty()
        {
            return peer.instance_name.clone();
        }

        if let Some(label) = ctx.session.target_label.as_deref()
            && !label.trim().is_empty()
        {
            return label.to_string();
        }

        peer_id
            .map(|peer_id| format!("Remote Peer - ID {}", truncate(peer_id, 8)))
            .unwrap_or_else(|| "Call in Progress".into())
    }
}
