use iced::{
    Alignment, Element, Length, Padding, padding,
    widget::{button, column, container, row, scrollable, text},
};
use iced_fonts::lucide;

use super::{SettingsMessage, SettingsScreen};
use crate::ui::{
    app::AppContext, components::vertical_spacer, fonts, message::Message, screens::settings::tabs,
    theme,
};

impl SettingsScreen {
    pub fn render_view<'a>(&'a self, ctx: &'a AppContext) -> Element<'a, Message> {
        let title = text("Settings").size(32).font(fonts::outfit::BOLD).style(text::primary);

        let tab_content = scrollable(self.active_tab.view(&self.working_config));

        let save_button = button(
            row![lucide::save(), text("Save").size(16)]
                .spacing(3)
                .padding(5)
                .align_y(Alignment::Center),
        )
        .padding(5)
        .on_press(Message::Settings(SettingsMessage::SaveSettings))
        .style(|theme, status| theme::button_style(theme, status, true));

        let discard_button = button(
            row![lucide::trash(), text("Discard").size(16)]
                .spacing(3)
                .padding(5)
                .align_y(Alignment::Center),
        )
        .padding(5)
        .on_press(Message::Settings(SettingsMessage::DiscardSettings))
        .style(|theme, status| theme::button_style(theme, status, false));

        let unsaved_changes_card = container(
            container(
                column![
                    row![
                        lucide::triangle_alert().size(18),
                        text("Unsaved Changes").size(16).font(fonts::outfit::BOLD),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    text("You have modified settings that haven't been saved yet.")
                        .size(13)
                        .style(text::secondary),
                    vertical_spacer(),
                    container(
                        row![save_button, discard_button].spacing(10).padding(padding::top(10))
                    )
                    .center_x(Length::Fill),
                ]
                .spacing(10),
            )
            .max_width(400)
            .padding(20)
            .style(theme::card_container),
        )
        .align_x(Alignment::Center)
        .align_y(Alignment::End)
        .padding(padding::top(20));

        let mut content = column![
            title,
            row![self.view_sidebar(), container(tab_content).width(Length::Fill)].spacing(10)
        ]
        .spacing(30);

        if self.working_config != ctx.config {
            content = content.push(unsaved_changes_card);
        }

        container(content).padding(20).width(Length::Fill).height(Length::Fill).into()
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        let tabs = tabs::TABS.iter().map(|(_name, tab)| {
            let is_active = &self.active_tab == tab;
            button(
                row![tab.icon().size(16), text(tab.label()).size(14)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(12)
            .on_press(Message::Settings(SettingsMessage::TabChanged(tab.clone())))
            .style(move |theme, status| theme::sidebar_button_style(theme, status, is_active))
            .into()
        });

        scrollable(
            container(column(tabs).spacing(5))
                .width(Length::Fixed(200.0))
                .height(Length::Fill)
                .padding(Padding::ZERO.right(20.0)),
        )
        .into()
    }
}
