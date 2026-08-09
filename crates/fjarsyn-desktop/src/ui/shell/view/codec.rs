use fjarsyn_engine::screen_share;
use iced::{
    Alignment, Element, Length,
    widget::{button, column, container, row, text},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message},
    theme,
};

pub(super) fn restart_required_banner<'a>(
    screen_share: &screen_share::Shares,
) -> Option<Element<'a, Message>> {
    let copy = restart_required_copy(
        screen_share.encoder_restart_required(),
        screen_share.decoder_restart_required(),
    )?;

    Some(
        container(
            row![
                lucide::triangle_alert().size(20),
                column![
                    text("Video restart required").size(14),
                    text(copy).size(12).style(text::secondary),
                    text(
                        "Messaging and current connections still work. Restarting will disconnect them."
                    )
                    .size(11)
                    .style(text::secondary),
                ]
                .spacing(3)
                .width(Length::Fill),
                button("Restart Fjarsyn")
                    .on_press(Message::Lifecycle(message::Lifecycle::RestartRequested))
                    .padding([8, 12]),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([10, 16])
        .style(theme::warning_accent_container)
        .into(),
    )
}

fn restart_required_copy(encoder: bool, decoder: bool) -> Option<&'static str> {
    match (encoder, decoder) {
        (true, true) => {
            Some("Sending and viewing screen shares are unavailable until Fjarsyn restarts.")
        }
        (true, false) => Some("Sending screen shares is unavailable until Fjarsyn restarts."),
        (false, true) => Some("Viewing screen shares is unavailable until Fjarsyn restarts."),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::restart_required_copy;

    #[test]
    fn restart_banner_copy_is_direction_specific_and_combines_failures() {
        assert_eq!(restart_required_copy(false, false), None);
        assert_eq!(
            restart_required_copy(true, false),
            Some("Sending screen shares is unavailable until Fjarsyn restarts.")
        );
        assert_eq!(
            restart_required_copy(false, true),
            Some("Viewing screen shares is unavailable until Fjarsyn restarts.")
        );
        assert_eq!(
            restart_required_copy(true, true),
            Some("Sending and viewing screen shares are unavailable until Fjarsyn restarts.")
        );
    }
}
