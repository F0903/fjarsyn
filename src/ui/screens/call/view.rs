use iced::{
    Alignment, Element, Length,
    widget::{Row, Space, button, container, stack, text},
};

use super::{CallMessage, CallScreen};
use crate::ui::{
    self,
    app::AppState,
    components::{CpuFrameViewer, GpuFrameViewer},
    message::{Message, NavigationMessage, Route, ScreenMessage},
};

impl CallScreen {
    pub fn render_view<'a>(&'a self, ctx: &'a AppState) -> Element<'a, Message> {
        let content = stack![
            self.view_remote_video(),
            self.view_local_preview(ctx),
            container(self.view_controls(ctx))
                .width(Length::Fill)
                .align_y(Alignment::End)
                .padding(20)
        ];

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(ui::theme::main_content_container)
            .into()
    }

    fn view_remote_video(&self) -> Element<'_, Message> {
        match self.remote_frame.clone() {
            Some(frame) => container(CpuFrameViewer::new(frame)).center(Length::Fill).into(),
            None => container(text("Waiting for video...").size(30)).center(Length::Fill).into(),
        }
    }

    fn view_local_preview(&self, ctx: &AppState) -> Element<'_, Message> {
        if let Some(local_frame) = self.local_frame.clone()
            && self.show_local_preview
            && ctx.config.enable_ui_preview
        {
            let viewer: Element<'_, Message> = match &local_frame.data {
                _ if local_frame.gpu_import_handle().is_some()
                    && crate::utils::gpu_preview::can_zero_copy_preview(local_frame.format) =>
                {
                    GpuFrameViewer::new(local_frame).into()
                }
                _ if local_frame.format.supports_software_preview() => {
                    CpuFrameViewer::new(local_frame).into()
                }
                _ => {
                    return container(
                        container(text("Preview unavailable for this format.").size(12))
                            .width(Length::Fixed(320.0))
                            .height(Length::Fixed(180.0))
                            .center(Length::Fill)
                            .style(container::bordered_box),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::End)
                    .align_y(Alignment::End)
                    .padding(20)
                    .into();
                }
            };

            container(
                container(viewer)
                    .width(Length::Fixed(320.0))
                    .height(Length::Fixed(180.0))
                    .style(container::bordered_box),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .padding(20)
            .into()
        } else {
            Space::new().into()
        }
    }

    fn view_controls(&self, ctx: &AppState) -> Element<'_, Message> {
        let mut controls_row = Row::new()
            .push(button("Settings").on_press(Message::Navigation(
                NavigationMessage::NavigateWithBack(Route::Settings),
            )))
            .spacing(10);

        controls_row = if self.capture.is_none() {
            controls_row.push(text("Screen sharing unavailable").size(14).style(text::secondary))
        } else if self.is_capturing() {
            let mut buttons = vec![
                button("Change Screen")
                    .on_press(Message::Screen(ScreenMessage::Call(CallMessage::StartCapture)))
                    .into(),
            ];
            if ctx.config.enable_ui_preview {
                buttons.push(
                    button(if self.show_local_preview { "Hide Preview" } else { "Show Preview" })
                        .on_press(Message::Screen(ScreenMessage::Call(
                            CallMessage::ToggleLocalPreview,
                        )))
                        .into(),
                );
            }
            buttons.push(
                button("Stop Sharing")
                    .style(iced::widget::button::danger)
                    .on_press(Message::Screen(ScreenMessage::Call(CallMessage::StopCapture)))
                    .into(),
            );
            controls_row.extend(buttons)
        } else {
            controls_row.extend([button("Share Screen")
                .on_press(Message::Screen(ScreenMessage::Call(CallMessage::StartCapture)))
                .into()])
        };

        controls_row = controls_row.extend([button("End Call")
            .style(iced::widget::button::danger)
            .on_press(Message::Screen(ScreenMessage::Call(CallMessage::EndCall)))
            .into()]);

        container(controls_row)
            .padding(15)
            .style(ui::theme::card_container)
            .width(Length::Shrink)
            .into()
    }
}
