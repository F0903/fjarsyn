use iced::{
    Alignment, Element, Length, Padding,
    widget::{Space, button, column, container, row, rule, scrollable, text},
};
use iced_fonts::lucide;

use super::{SettingsMessage, SettingsScreen};
use crate::ui::{
    fonts,
    message::{Message, ScreenMessage},
    screens::settings::tabs,
    shell::ShellContext,
    theme,
};

const SETTINGS_PAGE_MAX_WIDTH: f32 = 1380.0;
const SETTINGS_CONTENT_MAX_WIDTH: f32 = 940.0;
const SETTINGS_SIDEBAR_WIDTH: f32 = 220.0;

impl SettingsScreen {
    fn unsaved_changes_bar(&self) -> Element<'_, Message> {
        const SIZE: u32 = 13;

        let save_button = button(
            row![lucide::save().size(SIZE), text("Save").size(SIZE)]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding([6, 9])
        .on_press(Message::Screen(ScreenMessage::Settings(SettingsMessage::SaveSettings)))
        .style(|theme, status| theme::button_style(theme, status, true));

        let discard_button = button(
            row![lucide::trash().size(SIZE), text("Discard").size(SIZE)]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding([6, 9])
        .on_press(Message::Screen(ScreenMessage::Settings(SettingsMessage::DiscardSettings)))
        .style(|theme, status| theme::button_style(theme, status, false));

        container(
            row![
                row![
                    lucide::triangle_alert().size(SIZE),
                    text("Unsaved changes").size(SIZE).font(fonts::outfit::BOLD)
                ]
                .spacing(6),
                rule::vertical(1),
                row![discard_button, save_button].spacing(8).align_y(Alignment::Center),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .height(Length::Shrink)
        .padding([6, 8])
        .width(Length::Shrink)
        .style(theme::section_container)
        .into()
    }

    pub fn render_view<'a>(&'a self, ctx: ShellContext<'a>) -> Element<'a, Message> {
        let title = text("Settings").size(32).font(fonts::outfit::BOLD).style(text::primary);

        let tab_content = scrollable(
            container(self.active_tab.view(&self.working_config))
                .max_width(SETTINGS_CONTENT_MAX_WIDTH)
                .width(Length::Fill),
        );

        let unsaved_changes_bar = if self.working_config != ctx.config {
            self.unsaved_changes_bar()
        } else {
            Space::new().into()
        };

        let header_content = row![
            title,
            container(unsaved_changes_bar).width(Length::Fill).align_x(Alignment::End),
        ]
        .spacing(20)
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let content = column![
            header_content,
            row![
                self.view_sidebar(),
                container(tab_content).width(Length::Fill).padding(Padding::ZERO.left(8.0))
            ]
            .spacing(18)
        ]
        .spacing(24)
        .max_width(SETTINGS_PAGE_MAX_WIDTH);

        container(
            container(content)
                .padding(24)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        let tabs = tabs::TABS.values().map(|tab| {
            let is_active = &self.active_tab == tab;
            button(
                row![tab.icon().size(16), text(tab.label()).size(14)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(12)
            .on_press(Message::Screen(ScreenMessage::Settings(SettingsMessage::TabChanged(
                tab.clone(),
            ))))
            .style(move |theme, status| theme::sidebar_button_style(theme, status, is_active))
            .into()
        });

        container(
            column![
                text("Categories").size(12).font(fonts::outfit::SEMIBOLD).style(text::secondary),
                scrollable(container(column(tabs).spacing(6)).width(Length::Fill))
            ]
            .spacing(12),
        )
        .width(Length::Fixed(SETTINGS_SIDEBAR_WIDTH))
        .height(Length::Shrink)
        .padding(16)
        .style(theme::card_container)
        .into()
    }
}
