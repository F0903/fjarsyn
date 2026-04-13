use iced::{
    Alignment, Color, Element, Length, Shadow, Vector,
    widget::{Row, button, container, row, text},
};
use iced_fonts::lucide;

use super::{CallButtonTone, CallScreen, ControlSpec};
use crate::ui::{
    self, fonts,
    message::{Message, ScreenMessage},
    shell::ShellContext,
};

impl CallScreen {
    pub(super) fn view_controls(&self, ctx: ShellContext<'_>) -> Element<'_, Message> {
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

    fn control_specs(&self, ctx: ShellContext<'_>) -> Vec<ControlSpec<'static>> {
        let capture_busy = ctx.media.capture_initializing || self.capture.pending_start;
        let mut controls = vec![];

        if self.is_capturing() {
            controls.push(ControlSpec {
                label: "Change Screen",
                icon: lucide::video().size(14),
                action: Some(super::super::CallMessage::StartCapture),
                tone: CallButtonTone::Secondary,
            });
            controls.push(ControlSpec {
                label: if self.local.preview_visible { "Hide Preview" } else { "Show Preview" },
                icon: lucide::clapperboard().size(14),
                action: ctx
                    .config
                    .capture
                    .enable_ui_preview
                    .then_some(super::super::CallMessage::ToggleLocalPreview),
                tone: CallButtonTone::Secondary,
            });
            controls.push(ControlSpec {
                label: "Stop Sharing",
                icon: lucide::video().size(14),
                action: Some(super::super::CallMessage::StopCapture),
                tone: CallButtonTone::Danger,
            });
        } else {
            controls.push(ControlSpec {
                label: "Share Screen",
                icon: lucide::video().size(14),
                action: (!capture_busy).then_some(super::super::CallMessage::StartCapture),
                tone: CallButtonTone::Primary,
            });
        }

        controls.push(ControlSpec {
            label: "End Call",
            icon: lucide::phone_off().size(14),
            action: Some(super::super::CallMessage::EndCall),
            tone: CallButtonTone::Danger,
        });

        controls
    }
}
