use std::sync::Arc;

use fjarsyn_core::{
    media::{frame::Frame, gpu_interop, pixel_format::PixelFormat},
    utils::text::truncate,
};
use iced::{
    Alignment, Border, Color, Element, Length, Shadow, Vector,
    widget::{Row, Space, button, column, container, row, stack, text},
};
use iced_fonts::lucide;

use super::{CallMessage, CallScreen};
use crate::ui::{
    self,
    components::{CpuFrameViewer, GpuFrameViewer},
    fonts,
    message::{Message, ScreenMessage},
    shell::ShellContext,
};

const LOCAL_PREVIEW_WIDTH: f32 = 320.0;
const LOCAL_PREVIEW_HEIGHT: f32 = 180.0;
const FLOATING_OVERLAY_PADDING: u16 = 24;
const FLOATING_CARD_PADDING: u16 = 8;

enum CallButtonTone {
    Primary,
    Secondary,
    Danger,
}

struct WaitingStateCopy {
    title: &'static str,
    subtitle: &'static str,
    icon: iced::widget::Text<'static>,
}

struct ControlSpec<'a> {
    label: &'a str,
    icon: iced::widget::Text<'a>,
    action: Option<CallMessage>,
    tone: CallButtonTone,
}

impl CallScreen {
    pub fn render_view<'a>(&'a self, ctx: ShellContext<'a>) -> Element<'a, Message> {
        let content = stack![
            self.view_remote_video(ctx),
            self.view_local_preview(ctx),
            if ctx.ui.cursor_inside_window {
                container(self.view_controls(ctx))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::End)
                    .padding(FLOATING_OVERLAY_PADDING)
            } else {
                container(Space::new())
            }
        ];

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(ui::theme::main_content_container)
            .into()
    }

    fn view_remote_video(&self, ctx: ShellContext<'_>) -> Element<'_, Message> {
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

    fn view_local_preview(&self, ctx: ShellContext<'_>) -> Element<'_, Message> {
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

    fn view_controls(&self, ctx: ShellContext<'_>) -> Element<'_, Message> {
        let controls_row = self
            .control_specs(ctx)
            .into_iter()
            .fold(Row::new().spacing(10).align_y(Alignment::Center), |row, spec| {
                row.push(self.control_button(spec))
            });

        container(controls_row)
            .padding([16, 18])
            .style(|theme| {
                let mut style = ui::theme::card_container(theme);
                style.shadow = Shadow {
                    color: Color { a: 0.28, ..Color::BLACK },
                    offset: Vector::new(0.0, 8.0),
                    blur_radius: 22.0,
                };
                style
            })
            .width(Length::Shrink)
            .into()
    }

    fn control_button<'a>(&self, spec: ControlSpec<'a>) -> Element<'a, Message> {
        let mut button = button(
            row![spec.icon, text(spec.label).size(14).font(fonts::outfit::MEDIUM)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([10, 14])
        .width(Length::Shrink);

        if let Some(action) = spec.action {
            button = button.on_press(Message::Screen(ScreenMessage::Call(action)));
        }

        match spec.tone {
            CallButtonTone::Primary => {
                button.style(|theme, status| ui::theme::button_style(theme, status, true)).into()
            }
            CallButtonTone::Secondary => {
                button.style(|theme, status| ui::theme::button_style(theme, status, false)).into()
            }
            CallButtonTone::Danger => button.style(ui::theme::danger_button_style).into(),
        }
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

    fn control_specs(&self, ctx: ShellContext<'_>) -> Vec<ControlSpec<'static>> {
        let capture_busy = ctx.media.capture_initializing || self.capture.pending_start;
        let mut controls = vec![];

        if self.is_capturing() {
            controls.push(ControlSpec {
                label: "Change Screen",
                icon: lucide::video().size(14),
                action: Some(CallMessage::StartCapture),
                tone: CallButtonTone::Secondary,
            });
            controls.push(ControlSpec {
                label: if self.local.preview_visible { "Hide Preview" } else { "Show Preview" },
                icon: lucide::clapperboard().size(14),
                action: ctx
                    .config
                    .capture
                    .enable_ui_preview
                    .then_some(CallMessage::ToggleLocalPreview),
                tone: CallButtonTone::Secondary,
            });
            controls.push(ControlSpec {
                label: "Stop Sharing",
                icon: lucide::video().size(14),
                action: Some(CallMessage::StopCapture),
                tone: CallButtonTone::Danger,
            });
        } else {
            controls.push(ControlSpec {
                label: "Share Screen",
                icon: lucide::video().size(14),
                action: (!capture_busy).then_some(CallMessage::StartCapture),
                tone: CallButtonTone::Primary,
            });
        }

        controls.push(ControlSpec {
            label: "End Call",
            icon: lucide::phone_off().size(14),
            action: Some(CallMessage::EndCall),
            tone: CallButtonTone::Danger,
        });

        controls
    }

    fn preferred_frame_viewer(&self, frame: Arc<Frame>) -> Option<Element<'static, Message>> {
        if self.supports_zero_copy_preview(frame.format, frame.gpu_import_handle().is_some()) {
            Some(GpuFrameViewer::new(frame).into())
        } else if frame.format.supports_software_preview() {
            Some(CpuFrameViewer::new(frame).into())
        } else {
            None
        }
    }

    fn supports_zero_copy_preview(&self, pixel_format: PixelFormat, has_gpu_handle: bool) -> bool {
        has_gpu_handle && gpu_interop::supports_zero_copy_preview(pixel_format)
    }

    fn preview_surface<'a>(&self, content: Element<'a, Message>) -> Element<'a, Message> {
        container(content)
            .width(Length::Fixed(LOCAL_PREVIEW_WIDTH))
            .height(Length::Fixed(LOCAL_PREVIEW_HEIGHT))
            .style(container::bordered_box)
            .into()
    }

    fn view_preview_unavailable(&self) -> Element<'_, Message> {
        self.preview_surface(text("Preview unavailable for this format.").size(12).into())
    }
}
