use fjarsyn_engine::{
    config::Config,
    media::{
        codec::TranscodeType,
        video::{Framerate, TargetResolution},
    },
};
use iced::{
    Alignment, Element, Length,
    widget::{column, pick_list, row, scrollable, slider, text, text_input},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message},
    screens::settings::{
        components::{setting_row, settings_section},
        tabs::Tab,
    },
    theme,
};

#[derive(Debug)]
pub(super) struct Transcoding;

impl Tab for Transcoding {
    fn id(&self) -> message::screen::settings::TabId {
        message::screen::settings::TabId::Transcoding
    }

    fn label(&self) -> &'static str {
        "Transcoding"
    }

    fn icon(&self) -> iced::widget::Text<'static> {
        lucide::arrow_right_left()
    }

    fn view(&self, config: &Config) -> Element<'_, Message> {
        let content = column![
             settings_section(
                lucide::arrow_right_left(),
                "Transcoding",
                column![
                    setting_row(
                        "Transcoder",
                        "Video codec and preferred encode path. Decoding uses GPU acceleration when available.",
                        pick_list(TranscodeType::ALL, Some(config.video.transcoding_type), |val| {
                            Message::Screen(message::Screen::Settings(
                                message::screen::settings::Message::TranscodingTypeChanged(val),
                            ))
                        },)
                        .padding(8)
                        .width(Length::Fixed(200.0)),
                    ),
                     setting_row(
                        "Resolution",
                        "Maximum streaming resolution",
                        pick_list(TargetResolution::ALL, Some(config.video.target_resolution), |val| {
                            Message::Screen(message::Screen::Settings(
                                message::screen::settings::Message::TargetResolutionChanged(val),
                            ))
                        },)
                        .padding(8)
                        .width(Length::Fixed(200.0)),
                    ),
                    setting_row(
                        "Framerate",
                        "Target frames per second",
                        pick_list(Framerate::ALL, Some(config.video.target_framerate), |val| {
                            Message::Screen(message::Screen::Settings(
                                message::screen::settings::Message::TargetFramerateChanged(val),
                            ))
                        },)
                        .padding(8)
                        .width(Length::Fixed(200.0)),
                    ),
                    setting_row(
                        "Bitrate",
                        "Target bitrate for the video stream",
                        row![
                            slider(100_000..=100_000_000, config.video.target_bitrate, |val| {
                                Message::Screen(message::Screen::Settings(
                                    message::screen::settings::Message::TargetBitrateChanged(val),
                                ))
                            })
                            .width(Length::Fill),
                            text_input("Bitrate", &format!("{}", config.video.target_bitrate / 1000))
                                .on_input(|val| {
                                    Message::Screen(message::Screen::Settings(
                                        message::screen::settings::Message::TargetBitrateInputChanged(val),
                                    ))
                                })
                                .padding(8)
                                .width(Length::Fixed(80.0))
                                .style(theme::text_input_style),
                            text("kbps").size(12).style(text::secondary)
                        ]
                        .spacing(15)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                    ),
                ]
                .spacing(20),
            ),
        ]
        .spacing(25);

        scrollable(content).into()
    }
}
