use iced::{
    Alignment, Border, Color, Element, Length,
    widget::{Space, container, stack, text},
};

use super::{CallScreen, FLOATING_CARD_PADDING, FLOATING_OVERLAY_PADDING};
use crate::ui::{self, fonts, message::Message, shell::ShellContext};

impl CallScreen {
    pub(super) fn view_local_preview(&self, ctx: ShellContext<'_>) -> Element<'_, Message> {
        if let Some(local_frame) = self.local.latest_frame.clone()
            && self.local.preview_visible
            && ctx.config.capture.enable_ui_preview
        {
            let preview_content = self
                .preferred_frame_viewer(local_frame)
                .unwrap_or_else(|| self.view_preview_unavailable());

            let preview = stack![
                self.preview_surface(preview_content),
                container(
                    container(
                        text("You").size(11).font(fonts::outfit::SEMIBOLD).style(text::primary)
                    )
                    .padding([6, 10])
                    .style(|_| container::Style {
                        background: Some(Color { a: 0.85, ..ui::theme::CARD_BACKGROUND }.into()),
                        border: Border {
                            color: ui::theme::BORDER_COLOR,
                            width: 1.0,
                            radius: ui::theme::LIGHTER_RADIUS.into(),
                        },
                        ..Default::default()
                    }),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .padding(12),
            ];

            container(
                container(preview).padding(FLOATING_CARD_PADDING).style(ui::theme::card_container),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .padding(FLOATING_OVERLAY_PADDING)
            .into()
        } else {
            Space::new().into()
        }
    }
}
