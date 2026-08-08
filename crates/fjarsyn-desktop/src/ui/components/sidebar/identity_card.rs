use fjarsyn_engine::pairing::Invite;
use iced::{
    Alignment, Element, Length,
    widget::{button, column, container, row, text},
};
use iced_fonts::lucide;

use crate::ui::{
    message::Message,
    presentation::{Context, fingerprint_grid},
    theme,
};

pub(super) fn identity_card(context: Context<'_>) -> Element<'static, Message> {
    let pairing_invite = context
        .local_identity()
        .and_then(|(peer_id, public_key)| Invite::new(peer_id.clone(), public_key.to_owned()).ok());
    let id = context.local_peer_id().map(ToString::to_string);
    let id_text = id.clone().unwrap_or_else(|| "Starting...".into());
    let display = super::truncate_with_ellipsis(&id_text, 18);
    let fingerprint = pairing_invite.as_ref().map(|invite| invite.fingerprint().to_string());
    let fingerprint_display =
        fingerprint.as_deref().map(fingerprint_grid).unwrap_or_else(|| "Starting...".into());
    let invite_text = pairing_invite.map(|invite| invite.to_string());
    let mut copy_id = button(lucide::copy().size(14)).style(button::text);
    if let Some(id) = id {
        copy_id = copy_id.on_press(Message::CopyId(id));
    }
    let mut copy_invite = button(
        row![lucide::clipboard_copy().size(14), text("Copy pairing invite").size(11)]
            .spacing(7)
            .align_y(Alignment::Center),
    )
    .style(button::text);
    if let Some(invite) = invite_text {
        copy_invite = copy_invite.on_press(Message::CopyInvite(invite));
    }
    let mut copy_fingerprint = button(lucide::copy().size(14)).style(button::text);
    if let Some(fingerprint) = fingerprint {
        copy_fingerprint = copy_fingerprint.on_press(Message::CopyFingerprint(fingerprint));
    }

    container(
        column![
            row![
                column![
                    text("YOUR ID").size(10).style(text::secondary),
                    text(display).size(12).style(text::primary),
                ]
                .width(Length::Fill),
                copy_id,
            ]
            .align_y(Alignment::Center),
            row![
                text("FULL IDENTITY FINGERPRINT")
                    .size(10)
                    .style(text::secondary)
                    .width(Length::Fill),
                copy_fingerprint,
            ]
            .align_y(Alignment::Center),
            text(fingerprint_display)
                .size(12)
                .font(iced::Font::MONOSPACE)
                .style(text::primary)
                .width(Length::Fill),
            copy_invite,
            text("Copying is convenient, but compare through a separate trusted channel. Pairing is mutual: import theirs too.")
                .size(10)
                .style(text::secondary),
        ]
        .spacing(8),
    )
    .padding(10)
    .style(theme::id_card_container)
    .into()
}
