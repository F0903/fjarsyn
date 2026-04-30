use fjarsyn_core::services::contacts_service::Contact;
use iced::{
    Alignment, Element, Length, Padding,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use iced_fonts::lucide;

use super::{ContactsMessage, ContactsScreen};
use crate::ui::{
    fonts,
    message::{
        CallActionMessage, CallTarget, ContactsServiceMessage, Message, NavigationMessage, Route,
        ScreenMessage,
    },
    shell::ShellContext,
    theme,
};

impl ContactsScreen {
    pub fn render_view<'a>(&'a self, ctx: ShellContext<'a>) -> Element<'a, Message> {
        let mut content = column![self.view_header()].spacing(30);

        if self.show_add_form {
            content = content.push(self.view_add_contact_form());
        }

        let contacts = ctx.contacts.contacts.as_slice();
        content = content.push(container(self.view_contacts_list(contacts)).width(Length::Fill));

        container(scrollable(content)).width(Length::Fill).height(Length::Fill).padding(20).into()
    }

    fn view_header(&self) -> Element<'_, Message> {
        row![
            text("Contacts").size(32).style(text::primary).font(fonts::outfit::BOLD),
            container(
                button(
                    row![
                        if self.show_add_form {
                            lucide::minus().size(16)
                        } else {
                            lucide::plus().size(16)
                        },
                        text(if self.show_add_form { "Close Form" } else { "Add Contact" })
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center)
                )
                .on_press(Message::Screen(ScreenMessage::Contacts(ContactsMessage::ToggleAddForm)))
                .padding(10)
                .style(|theme, status| theme::button_style(
                    theme,
                    status,
                    !self.show_add_form
                ))
            )
            .width(Length::Fill)
            .align_x(Alignment::End)
        ]
        .spacing(20)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
    }

    fn view_add_contact_form(&self) -> Element<'_, Message> {
        container(
            column![
                text("Add New Contact").size(18),
                row![
                    column![
                        text("Name").size(12).style(text::secondary),
                        text_input("John Doe", &self.new_contact_name)
                            .on_input(|val| {
                                Message::Screen(ScreenMessage::Contacts(
                                    ContactsMessage::NameChanged(val),
                                ))
                            })
                            .padding(10)
                            .style(theme::text_input_style),
                    ]
                    .spacing(5)
                    .width(Length::FillPortion(1)),
                    column![
                        text("Peer ID").size(12).style(text::secondary),
                        text_input("Enter Peer ID...", &self.new_contact_peer_id)
                            .on_input(|val| {
                                Message::Screen(ScreenMessage::Contacts(
                                    ContactsMessage::PeerIdChanged(val),
                                ))
                            })
                            .padding(10)
                            .style(theme::text_input_style),
                    ]
                    .spacing(5)
                    .width(Length::FillPortion(1)),
                    column![
                        text("Address (Optional)").size(12).style(text::secondary),
                        text_input("192.168.1.50:8080", &self.new_contact_address)
                            .on_input(|val| {
                                Message::Screen(ScreenMessage::Contacts(
                                    ContactsMessage::AddressChanged(val),
                                ))
                            })
                            .padding(10)
                            .style(theme::text_input_style),
                    ]
                    .spacing(5)
                    .width(Length::FillPortion(1)),
                ]
                .spacing(15)
                .align_y(Alignment::Start),
                row![
                    column![
                        text("Trusted Public Key").size(12).style(text::secondary),
                        text_input(
                            "Paste peer signaling public key",
                            &self.new_contact_trusted_public_key
                        )
                        .on_input(|val| {
                            Message::Screen(ScreenMessage::Contacts(
                                ContactsMessage::TrustedPublicKeyChanged(val),
                            ))
                        })
                        .padding(10)
                        .style(theme::text_input_style),
                    ]
                    .spacing(5)
                    .width(Length::Fill),
                    container(
                        button(row![lucide::user_plus().size(16), text("Save")].spacing(10))
                            .on_press(Message::Screen(ScreenMessage::Contacts(
                                ContactsMessage::AddNewContact,
                            )))
                            .padding(10)
                            .width(Length::Fixed(120.0))
                            .style(|theme, status| theme::button_style(theme, status, true))
                    )
                    .padding(Padding::ZERO.top(17.0))
                ]
                .spacing(15)
                .align_y(Alignment::Start),
            ]
            .spacing(15),
        )
        .padding(20)
        .style(crate::ui::theme::card_container)
        .width(Length::Fill)
        .into()
    }

    fn view_contacts_list<'a>(&'a self, contacts: &[Contact]) -> Element<'a, Message> {
        let mut list = column![text("Saved Contacts").size(18)].spacing(15);

        if contacts.is_empty() {
            list = list.push(
                container(text("No contacts saved yet.").size(14).style(text::secondary))
                    .padding(20)
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
            );
        } else {
            for contact in contacts {
                let contact_id = contact.id;
                let contact_peer_id = contact.peer_id.clone();
                let contact_name = contact.name.clone();
                let trusted_key_editor = self.view_trusted_key_editor(contact);

                let contact_card = container(
                    row![
                        container(lucide::user().size(20).center())
                            .padding(8)
                            .style(crate::ui::theme::icon_bubble_container),
                        column![
                            text(contact_name).size(16),
                            text(format!("ID: {}", contact_peer_id))
                                .size(12)
                                .style(text::secondary),
                            trusted_key_editor,
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        row![
                            button(lucide::message_square().size(16))
                                .on_press(Message::Navigation(NavigationMessage::Navigate(
                                    Route::Messages { peer_id: Some(contact_peer_id.clone()) },
                                )))
                                .padding(8)
                                .style(|theme, status| theme::button_style(theme, status, false)),
                            button(lucide::phone().size(16))
                                .on_press(Message::CallAction(CallActionMessage::StartCall(
                                    CallTarget::ContactId(contact_id),
                                )))
                                .padding(8)
                                .style(|theme, status| theme::button_style(theme, status, false)),
                            button(lucide::trash().size(16))
                                .on_press(Message::ContactData(
                                    ContactsServiceMessage::DeleteContact(contact_id)
                                ))
                                .padding(8)
                                .style(theme::danger_button_style),
                        ]
                        .spacing(8)
                    ]
                    .spacing(15)
                    .align_y(Alignment::Center),
                )
                .padding(15)
                .style(crate::ui::theme::card_container);

                list = list.push(contact_card);
            }
        }

        list.into()
    }

    fn view_trusted_key_editor<'a>(&'a self, contact: &Contact) -> Element<'a, Message> {
        if self.editing_contact_id == Some(contact.id) {
            return row![
                text_input("Paste peer signaling public key", &self.editing_trusted_public_key)
                    .on_input(|val| {
                        Message::Screen(ScreenMessage::Contacts(
                            ContactsMessage::ExistingTrustedPublicKeyChanged(val),
                        ))
                    })
                    .padding(8)
                    .style(theme::text_input_style)
                    .width(Length::Fill),
                button(row![lucide::save().size(14), text("Save").size(13)])
                    .on_press(Message::Screen(ScreenMessage::Contacts(
                        ContactsMessage::SaveTrustedPublicKeyEdit,
                    )))
                    .padding(8)
                    .style(|theme, status| theme::button_style(theme, status, true)),
                button(lucide::x().size(14))
                    .on_press(Message::Screen(ScreenMessage::Contacts(
                        ContactsMessage::CancelTrustedPublicKeyEdit,
                    )))
                    .padding(8)
                    .style(|theme, status| theme::button_style(theme, status, false)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into();
        }

        let current_key = contact.trusted_public_key.clone();
        row![
            text(trusted_key_status(current_key.as_deref())).size(12).style(text::secondary),
            button(text("Edit Key").size(12))
                .on_press(Message::Screen(ScreenMessage::Contacts(
                    ContactsMessage::StartTrustedPublicKeyEdit { id: contact.id, current_key },
                )))
                .padding([4, 8])
                .style(|theme, status| theme::button_style(theme, status, false)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }
}

fn trusted_key_status(trusted_public_key: Option<&str>) -> &'static str {
    if trusted_public_key.is_some() { "trusted key saved" } else { "missing trusted key" }
}
