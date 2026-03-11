use iced::{
    Alignment, Element, Length, Subscription, Task,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use iced_fonts::lucide;

use super::Screen;
use crate::ui::{
    fonts,
    message::{CallTarget, Message},
    state::AppContext,
    theme,
};

#[derive(Debug, Clone)]
pub enum ContactsMessage {
    NameChanged(String),
    PeerIdChanged(String),
    AddressChanged(String),
    ToggleAddForm,
    SubmitAdd,
}

#[derive(Debug, Clone)]
pub struct ContactsScreen {
    show_add_form: bool,
    new_name: String,
    new_peer_id: String,
    new_address: String,
}

impl ContactsScreen {
    pub fn new(_ctx: &mut AppContext) -> Self {
        Self {
            show_add_form: false,
            new_name: String::new(),
            new_peer_id: String::new(),
            new_address: String::new(),
        }
    }
}

impl Screen for ContactsScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, _ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::Contacts(msg) => match msg {
                ContactsMessage::NameChanged(val) => {
                    self.new_name = val;
                    Task::none()
                }
                ContactsMessage::PeerIdChanged(val) => {
                    self.new_peer_id = val;
                    Task::none()
                }
                ContactsMessage::AddressChanged(val) => {
                    self.new_address = val;
                    Task::none()
                }
                ContactsMessage::ToggleAddForm => {
                    self.show_add_form = !self.show_add_form;
                    Task::none()
                }
                ContactsMessage::SubmitAdd => {
                    if self.new_name.is_empty() || self.new_peer_id.is_empty() {
                        return Task::done(Message::NotifyError(
                            "Name and Peer ID are required.".to_string(),
                        ));
                    }

                    let address = if self.new_address.is_empty() {
                        None
                    } else {
                        Some(self.new_address.clone())
                    };

                    let task = Task::done(Message::SaveContact {
                        peer_id: self.new_peer_id.clone(),
                        name: self.new_name.clone(),
                        address,
                    });

                    self.new_name.clear();
                    self.new_peer_id.clear();
                    self.new_address.clear();
                    self.show_add_form = false;

                    task
                }
            },
            Message::ContactSaved(Ok(_)) | Message::ContactDeleted(Ok(_)) => {
                // Refresh is handled by the main handler (LoadContacts)
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view<'a>(&'a self, ctx: &'a AppContext) -> Element<'a, Message> {
        let title = text("Contacts").size(32).style(text::primary).font(fonts::outfit::BOLD);

        let header = row![
            title,
            container(
                button(row![lucide::user_plus().size(16), text("Add Contact")].spacing(10))
                    .on_press(Message::Contacts(ContactsMessage::ToggleAddForm))
                    .padding(10)
                    .style(|theme, status| theme::button_style(theme, status, true))
            )
            .width(Length::Fill)
            .align_x(Alignment::End)
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let mut content = column![header].spacing(30);

        if self.show_add_form {
            let add_form = container(
                column![
                    text("Add New Contact").size(18),
                    column![
                        text("Name").size(14).style(text::secondary),
                        text_input("John Doe", &self.new_name)
                            .on_input(|v| Message::Contacts(ContactsMessage::NameChanged(v)))
                            .padding(10)
                            .style(theme::text_input_style),
                    ]
                    .spacing(5),
                    column![
                        text("Peer ID").size(14).style(text::secondary),
                        text_input("uuid...", &self.new_peer_id)
                            .on_input(|v| Message::Contacts(ContactsMessage::PeerIdChanged(v)))
                            .padding(10)
                            .style(theme::text_input_style),
                    ]
                    .spacing(5),
                    column![
                        text("Direct Address (Optional)").size(14).style(text::secondary),
                        text_input("192.168.1.50:8080", &self.new_address)
                            .on_input(|v| Message::Contacts(ContactsMessage::AddressChanged(v)))
                            .padding(10)
                            .style(theme::text_input_style),
                    ]
                    .spacing(5),
                    row![
                        button(text("Cancel"))
                            .on_press(Message::Contacts(ContactsMessage::ToggleAddForm))
                            .padding(10)
                            .style(|theme, status| theme::button_style(theme, status, false)),
                        button(text("Save Contact"))
                            .on_press(Message::Contacts(ContactsMessage::SubmitAdd))
                            .padding(10)
                            .style(|theme, status| theme::button_style(theme, status, true)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                ]
                .spacing(15),
            )
            .padding(20)
            .style(crate::ui::theme::card_container);

            content = content.push(add_form);
        }

        if ctx.contacts.is_empty() {
            content = content.push(
                container(
                    column![
                        lucide::users().size(48).style(text::secondary),
                        text("No contacts yet.").size(16).style(text::secondary),
                        text("Add some friends to start sharing!").size(14).style(text::secondary),
                    ]
                    .spacing(10)
                    .align_x(Alignment::Center),
                )
                .width(Length::Fill)
                .padding(50)
                .center_x(Length::Fill),
            );
        } else {
            let mut contact_list = column![].spacing(10);

            for contact in &ctx.contacts {
                let is_online = ctx.discovered_peers.iter().any(|p| p.id == contact.peer_id);

                let status_dot =
                    container(Space::new().width(8)).width(8).height(8).style(move |_| {
                        container::Style {
                            background: Some(
                                if is_online {
                                    iced::Color::from_rgb(0.2, 0.8, 0.2)
                                } else {
                                    iced::Color::from_rgb(0.5, 0.5, 0.5)
                                }
                                .into(),
                            ),
                            border: iced::Border { radius: 4.0.into(), ..Default::default() },
                            ..Default::default()
                        }
                    });

                let contact_card = container(
                    row![
                        // Icon bubble
                        container(container(lucide::user().size(20).center()).center(Length::Fill))
                            .padding(6)
                            .style(crate::ui::theme::icon_bubble_container)
                            .width(Length::Fixed(44.0))
                            .height(Length::Fixed(44.0)),
                        // Content area
                        container(
                            row![
                                column![
                                    row![
                                        text(&contact.name).size(16).font(fonts::outfit::SEMIBOLD),
                                        status_dot,
                                    ]
                                    .spacing(8)
                                    .align_y(Alignment::Center),
                                    text(format!(
                                        "ID: {}",
                                        crate::utils::string_utils::truncate(&contact.peer_id, 8)
                                    ))
                                    .size(12)
                                    .style(text::secondary),
                                ]
                                .spacing(2)
                                .width(Length::Fill),
                                row![
                                    action_button(
                                        lucide::phone(),
                                        Message::StartCall(CallTarget::ContactId(contact.id)),
                                        true
                                    ),
                                    action_button(
                                        lucide::trash(),
                                        Message::DeleteContact(contact.id),
                                        false
                                    ),
                                ]
                                .spacing(8)
                            ]
                            .spacing(15)
                            .align_y(Alignment::Center)
                        )
                        .width(Length::Fill)
                    ]
                    .spacing(15)
                    .align_y(Alignment::Center),
                )
                .padding(12)
                .style(crate::ui::theme::card_container);

                contact_list = contact_list.push(contact_card);
            }

            content = content.push(scrollable(contact_list));
        }

        container(content).width(Length::Fill).height(Length::Fill).padding(20).into()
    }
}

fn action_button<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    is_primary: bool,
) -> iced::widget::Button<'a, Message> {
    button(container(icon.size(16)).center_x(Length::Fill).center_y(Length::Fill))
        .on_press(msg)
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0))
        .style(move |theme, status| theme::button_style(theme, status, is_primary))
}

use iced::widget::Space;
