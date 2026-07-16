use fjarsyn_core::{
    pairing::PairingInvite, peer_session::PeerSessionPhase, services::contacts_service::Contact,
};
use iced::{
    Alignment, Element, Length, Padding,
    widget::{button, checkbox, column, container, row, scrollable, text, text_input},
};
use iced_fonts::lucide;

use super::{ContactsMessage, ContactsScreen};
use crate::ui::{
    fonts,
    message::{Message, NavigationMessage, Route, ScreenMessage},
    presentation::project_peer,
    shell::ShellContext,
    theme,
};

const FINGERPRINT_CONFIRMATION: &str =
    "I compared the entire fingerprint over an independent trusted channel.";

impl ContactsScreen {
    pub fn render_view<'a>(&'a self, ctx: ShellContext<'a>) -> Element<'a, Message> {
        let mut content = column![self.view_header()].spacing(30);

        if self.show_add_form {
            content = content.push(self.view_add_contact_form(ctx.local_peer_id.is_some()));
        }

        content = content.push(container(self.view_contacts_list(ctx)).width(Length::Fill));

        container(scrollable(content)).width(Length::Fill).height(Length::Fill).padding(20).into()
    }

    fn view_header(&self) -> Element<'_, Message> {
        let mut form_button = button(
            row![
                if self.show_add_form { lucide::minus().size(16) } else { lucide::plus().size(16) },
                text(if self.show_add_form { "Close Form" } else { "Add Contact" })
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(10)
        .style(|theme, status| theme::button_style(theme, status, !self.show_add_form));
        if self.saving_new_contact.is_none() {
            form_button = form_button
                .on_press(Message::Screen(ScreenMessage::Contacts(ContactsMessage::ToggleAddForm)));
        }

        row![
            column![
                text("Contacts").size(32).style(text::primary).font(fonts::outfit::BOLD),
                text("Pairing is mutual: both people must import the other person's invite.")
                    .size(12)
                    .style(text::secondary),
            ]
            .spacing(5),
            container(form_button).width(Length::Fill).align_x(Alignment::End)
        ]
        .spacing(20)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
    }

    fn view_add_contact_form(&self, local_identity_ready: bool) -> Element<'_, Message> {
        let pairing = &self.new_contact_pairing;
        let reading = pairing.is_reading_clipboard();
        let mut invite_input =
            text_input("Paste the complete fjarsyn:pair invite", &pairing.invite_text)
                .padding(10)
                .style(theme::text_input_style)
                .width(Length::Fill);
        if self.saving_new_contact.is_none() {
            invite_input = invite_input.on_input(|value| {
                Message::Screen(ScreenMessage::Contacts(ContactsMessage::NewInviteChanged(value)))
            });
        }

        let mut paste_button = button(
            row![
                lucide::clipboard_copy().size(16),
                text(if reading { "Reading..." } else { "Paste invite" })
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(10)
        .style(|theme, status| theme::button_style(theme, status, false));
        if !reading && self.saving_new_contact.is_none() {
            paste_button = paste_button.on_press(Message::Screen(ScreenMessage::Contacts(
                ContactsMessage::PasteNewInvite,
            )));
        }

        let can_save = local_identity_ready
            && self.saving_new_contact.is_none()
            && !self.new_contact_name.trim().is_empty()
            && pairing.is_ready();
        let mut save_button = button(
            row![
                lucide::user_plus().size(16),
                text(if self.saving_new_contact.is_some() { "Saving..." } else { "Save contact" })
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(10)
        .width(Length::Fixed(150.0))
        .style(|theme, status| theme::button_style(theme, status, true));
        if can_save {
            save_button = save_button
                .on_press(Message::Screen(ScreenMessage::Contacts(ContactsMessage::AddNewContact)));
        }

        let mut name_input =
            text_input("Alice", &self.new_contact_name).padding(10).style(theme::text_input_style);
        if self.saving_new_contact.is_none() {
            name_input = name_input.on_input(|value| {
                Message::Screen(ScreenMessage::Contacts(ContactsMessage::NameChanged(value)))
            });
        }

        let mut fields = column![
            text("Add trusted contact").size(18),
            text(
                "Ask the other person for their pairing invite. Compare the fingerprint through a separate trusted channel before saving."
            )
            .size(12)
            .style(text::secondary),
            column![
                text("Name on this device").size(12).style(text::secondary),
                name_input,
            ]
            .spacing(5),
            column![
                text("Pairing invite").size(12).style(text::secondary),
                row![invite_input, paste_button].spacing(10).align_y(Alignment::Center),
            ]
            .spacing(5),
            self.view_new_pairing_verification(),
        ]
        .spacing(15);

        if !local_identity_ready {
            fields = fields.push(error_text("Your local identity is still starting."));
        }

        container(
            column![fields, container(save_button).width(Length::Fill).align_x(Alignment::End)]
                .spacing(15),
        )
        .padding(20)
        .style(theme::card_container)
        .width(Length::Fill)
        .into()
    }

    fn view_new_pairing_verification(&self) -> Element<'_, Message> {
        let pairing = &self.new_contact_pairing;
        let Some(invite) = pairing.invite.as_ref() else {
            return pairing.error.as_deref().map(error_text).unwrap_or_else(|| {
                text("The invite preview will appear here.").size(12).style(text::secondary).into()
            });
        };

        let mut confirmation =
            checkbox(pairing.fingerprint_confirmed).label(FINGERPRINT_CONFIRMATION);
        if self.saving_new_contact.is_none() {
            confirmation = confirmation.on_toggle(|confirmed| {
                Message::Screen(ScreenMessage::Contacts(ContactsMessage::NewFingerprintConfirmed(
                    confirmed,
                )))
            });
        }

        column![pairing_preview(invite), confirmation].spacing(12).into()
    }

    fn view_contacts_list<'a>(&'a self, ctx: ShellContext<'a>) -> Element<'a, Message> {
        let mut list = column![text("Saved Contacts").size(18)].spacing(15);

        if ctx.contacts.is_empty() {
            list = list.push(
                container(text("No contacts saved yet.").size(14).style(text::secondary))
                    .padding(20)
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
            );
        } else {
            for contact in ctx.contacts.iter() {
                let contact_id = contact.id;
                let contact_peer_id = contact.peer_id.clone();
                let nearby = ctx.is_nearby(&contact_peer_id);
                let phase =
                    ctx.sessions.session_for_peer(&contact_peer_id).map(|session| session.phase);
                let presentation = project_peer(nearby, phase);
                let deletion = self
                    .contact_deletion
                    .as_ref()
                    .filter(|deletion| deletion.contact_id == contact_id);
                let delete_requested = deletion.is_some();
                let delete_in_flight =
                    deletion.is_some_and(|deletion| deletion.operation_id.is_some());
                let identity_open = self
                    .identity_replacement
                    .as_ref()
                    .is_some_and(|replacement| replacement.contact_id == contact_id);
                let delete_workflow_available = self.contact_deletion.is_none() || delete_requested;
                let live_session_blocks_trust = !presentation.can_mutate_trust();
                let can_delete =
                    presentation.can_mutate_trust() && !identity_open && delete_workflow_available;
                let identity_editor = self.view_identity_editor(
                    contact,
                    presentation.can_mutate_trust() && !delete_requested,
                );
                let session_label = phase
                    .map(|phase| match phase {
                        PeerSessionPhase::Requesting | PeerSessionPhase::Negotiating => {
                            "Connecting"
                        }
                        PeerSessionPhase::Incoming => "Incoming",
                        PeerSessionPhase::Connected => "Connected",
                        PeerSessionPhase::Disconnecting => "Disconnecting",
                    })
                    .unwrap_or("Disconnected");

                let open_button = button(
                    row![lucide::panel_right_open().size(15), text("Open").size(13)].spacing(7),
                )
                .on_press(Message::Navigation(NavigationMessage::Navigate(Route::Peer {
                    peer_id: contact_peer_id.clone(),
                })))
                .padding(8)
                .style(|theme, status| theme::button_style(theme, status, false));
                let delete_actions: Element<'_, Message> = if delete_requested {
                    let mut confirm = button(text("Confirm delete").size(12))
                        .padding(8)
                        .style(theme::danger_button_style);
                    if can_delete && !delete_in_flight {
                        confirm = confirm.on_press(Message::Screen(ScreenMessage::Contacts(
                            ContactsMessage::ConfirmDeleteContact(contact_id),
                        )));
                    }
                    let mut cancel = button(text("Cancel").size(12))
                        .padding(8)
                        .style(|theme, status| theme::button_style(theme, status, false));
                    if !delete_in_flight {
                        cancel = cancel.on_press(Message::Screen(ScreenMessage::Contacts(
                            ContactsMessage::CancelDeleteContact,
                        )));
                    }
                    let actions = row![
                        if delete_in_flight {
                            button(text("Deleting...").size(12))
                                .padding(8)
                                .style(theme::danger_button_style)
                        } else {
                            confirm
                        },
                        cancel,
                    ]
                    .spacing(6);
                    if live_session_blocks_trust {
                        column![
                            text("Disconnect before deleting this identity.")
                                .size(11)
                                .style(text::secondary),
                            actions,
                        ]
                        .spacing(4)
                        .into()
                    } else {
                        actions.into()
                    }
                } else {
                    let mut delete_button = button(lucide::trash().size(16))
                        .padding(8)
                        .style(theme::danger_button_style);
                    if can_delete {
                        delete_button =
                            delete_button.on_press(Message::Screen(ScreenMessage::Contacts(
                                ContactsMessage::RequestDeleteContact(contact_id),
                            )));
                    }
                    delete_button.into()
                };

                let contact_card = container(
                    column![
                        row![
                            container(lucide::user().size(20).center())
                                .padding(8)
                                .style(theme::icon_bubble_container),
                            column![
                                text(contact.name.clone()).size(16),
                                row![
                                    text(format!("Peer ID: {}", contact.peer_id))
                                        .size(12)
                                        .font(iced::Font::MONOSPACE)
                                        .style(text::secondary)
                                        .width(Length::Fill)
                                        .wrapping(text::Wrapping::WordOrGlyph),
                                    button(lucide::copy().size(13))
                                        .on_press(Message::CopyId(contact.peer_id.to_string()))
                                        .style(button::text),
                                ]
                                .align_y(Alignment::Start),
                                text(format!(
                                    "Presence: {}  |  Session: {}",
                                    if nearby { "Nearby" } else { "Away" },
                                    session_label,
                                ))
                                .size(11)
                                .style(text::secondary),
                            ]
                            .spacing(2)
                            .width(Length::Fill),
                            row![open_button, delete_actions].spacing(8),
                        ]
                        .spacing(15)
                        .align_y(Alignment::Center),
                        identity_editor,
                    ]
                    .spacing(12),
                )
                .padding(15)
                .style(theme::card_container);

                list = list.push(contact_card);
            }
        }

        list.into()
    }

    fn view_identity_editor<'a>(
        &'a self,
        contact: &Contact,
        trust_mutation_allowed: bool,
    ) -> Element<'a, Message> {
        let stored_fingerprint = stored_fingerprint(contact);
        if let Some(replacement) = self
            .identity_replacement
            .as_ref()
            .filter(|replacement| replacement.contact_id == contact.id)
        {
            let pairing = &replacement.pairing;
            let reading = pairing.is_reading_clipboard();
            let mut invite_input = text_input(
                "Paste a replacement invite for this exact Peer ID",
                &pairing.invite_text,
            )
            .padding(8)
            .style(theme::text_input_style)
            .width(Length::Fill);
            if replacement.saving.is_none() && trust_mutation_allowed {
                invite_input = invite_input.on_input(|value| {
                    Message::Screen(ScreenMessage::Contacts(
                        ContactsMessage::ReplacementInviteChanged(value),
                    ))
                });
            }

            let mut paste_button = button(
                row![
                    lucide::clipboard_copy().size(14),
                    text(if reading { "Reading..." } else { "Paste invite" }).size(12)
                ]
                .spacing(7),
            )
            .padding(8)
            .style(|theme, status| theme::button_style(theme, status, false));
            if !reading && replacement.saving.is_none() && trust_mutation_allowed {
                paste_button = paste_button.on_press(Message::Screen(ScreenMessage::Contacts(
                    ContactsMessage::PasteReplacementInvite,
                )));
            }

            let mut verification = column![].spacing(10);
            if let Some(invite) = pairing.invite.as_ref() {
                verification = verification.push(pairing_preview(invite));
                let mut confirmation =
                    checkbox(pairing.fingerprint_confirmed).label(FINGERPRINT_CONFIRMATION);
                if trust_mutation_allowed && replacement.saving.is_none() {
                    confirmation = confirmation.on_toggle(|confirmed| {
                        Message::Screen(ScreenMessage::Contacts(
                            ContactsMessage::ReplacementFingerprintConfirmed(confirmed),
                        ))
                    });
                }
                verification = verification.push(confirmation);
            } else if let Some(error) = pairing.error.as_deref() {
                verification = verification.push(error_text(error));
            }

            let can_save =
                trust_mutation_allowed && replacement.saving.is_none() && pairing.is_ready();
            let mut save_button = button(
                row![
                    lucide::shield_check().size(14),
                    text(if replacement.saving.is_some() {
                        "Saving..."
                    } else {
                        "Replace identity"
                    })
                    .size(13)
                ]
                .spacing(7),
            )
            .padding(8)
            .style(|theme, status| theme::button_style(theme, status, true));
            if can_save {
                save_button = save_button.on_press(Message::Screen(ScreenMessage::Contacts(
                    ContactsMessage::SaveIdentityReplacement,
                )));
            }
            let mut cancel_button =
                button(row![lucide::x().size(14), text("Cancel").size(13)].spacing(7))
                    .padding(8)
                    .style(|theme, status| theme::button_style(theme, status, false));
            if replacement.saving.is_none() {
                cancel_button = cancel_button.on_press(Message::Screen(ScreenMessage::Contacts(
                    ContactsMessage::CancelIdentityReplacement,
                )));
            }

            return container(
                {
                    let mut editor = column![
                    fingerprint_block("Current identity fingerprint", &stored_fingerprint),
                    text(format!(
                        "Replacement must contain the exact Peer ID {}. Compare its new fingerprint independently.",
                        contact.peer_id
                    ))
                    .size(12)
                    .style(text::secondary)
                    .width(Length::Fill)
                    .wrapping(text::Wrapping::WordOrGlyph),
                    row![invite_input, paste_button].spacing(8).align_y(Alignment::Center),
                    verification,
                    row![save_button, cancel_button].spacing(8),
                    ]
                    .spacing(10);
                    if !trust_mutation_allowed {
                        editor = editor.push(
                            text("Disconnect before replacing this identity.")
                                .size(11)
                                .style(text::secondary),
                        );
                    }
                    editor
                },
            )
            .padding(Padding::from([12, 0]))
            .into();
        }

        let mut replace_button = button(text("Replace Identity Invite").size(12))
            .padding([5, 9])
            .style(|theme, status| theme::button_style(theme, status, false));
        if trust_mutation_allowed && self.identity_replacement.is_none() {
            replace_button = replace_button.on_press(Message::Screen(ScreenMessage::Contacts(
                ContactsMessage::StartIdentityReplacement {
                    id: contact.id,
                    peer_id: contact.peer_id.clone(),
                },
            )));
        }

        column![
            fingerprint_block("Identity fingerprint", &stored_fingerprint),
            row![
                replace_button,
                if trust_mutation_allowed {
                    text("Replacing an identity disconnects any existing session.")
                        .size(11)
                        .style(text::secondary)
                } else {
                    text("Disconnect before replacing or deleting this identity.")
                        .size(11)
                        .style(text::secondary)
                },
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(7)
        .into()
    }
}

fn pairing_preview(invite: &PairingInvite) -> Element<'_, Message> {
    let fingerprint = invite.fingerprint().to_string();
    let peer_id = invite.peer_id().to_string();
    container(
        column![
            row![
                text("Exact Peer ID").size(11).style(text::secondary).width(Length::Fixed(135.0)),
                text(peer_id.clone())
                    .size(12)
                    .font(iced::Font::MONOSPACE)
                    .width(Length::Fill)
                    .wrapping(text::Wrapping::WordOrGlyph),
                button(lucide::copy().size(14))
                    .on_press(Message::CopyId(peer_id))
                    .style(button::text),
            ]
            .spacing(10)
            .align_y(Alignment::Start),
            fingerprint_block("Full identity fingerprint", &fingerprint),
            text("Copying is only a convenience; compare the complete value over the independent trusted channel before confirming.")
                .size(11)
                .style(text::secondary),
        ]
        .spacing(8),
    )
    .padding(12)
    .width(Length::Fill)
    .style(theme::id_card_container)
    .into()
}

fn fingerprint_block<'a>(label: &'a str, fingerprint: &str) -> Element<'a, Message> {
    column![
        row![
            text(label).size(11).style(text::secondary).width(Length::Fill),
            button(lucide::copy().size(14))
                .on_press(Message::CopyFingerprint(fingerprint.to_owned()))
                .style(button::text),
        ]
        .align_y(Alignment::Center),
        text(fingerprint_grid(fingerprint))
            .size(13)
            .font(iced::Font::MONOSPACE)
            .width(Length::Fill),
    ]
    .spacing(5)
    .into()
}

fn fingerprint_grid(fingerprint: &str) -> String {
    fingerprint
        .split_whitespace()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|groups| groups.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn stored_fingerprint(contact: &Contact) -> String {
    PairingInvite::new(contact.peer_id.clone(), contact.trusted_public_key.clone())
        .map(|invite| invite.fingerprint().to_string())
        .unwrap_or_else(|_| "Invalid stored identity".into())
}

fn error_text(message: &str) -> Element<'_, Message> {
    text(message).size(12).color(iced::Color::from_rgb(0.92, 0.34, 0.34)).into()
}
