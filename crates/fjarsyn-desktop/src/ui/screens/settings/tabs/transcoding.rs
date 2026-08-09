use fjarsyn_engine::media::{
    codec::TranscodeType,
    video::{Framerate, TargetResolution},
};
use iced::{
    Alignment, Element, Length,
    widget::{column, pick_list, row, scrollable, slider, text, text_input},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message},
    screens::settings::{
        SettingsDraft,
        components::{setting_row, settings_section},
        settings_draft::{MAX_TARGET_BITRATE_KBPS, MIN_TARGET_BITRATE_KBPS},
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

    fn view<'a>(&self, draft: &'a SettingsDraft) -> Element<'a, Message> {
        let bitrate = row![
            slider(
                MIN_TARGET_BITRATE_KBPS..=MAX_TARGET_BITRATE_KBPS,
                draft.video.target_bitrate_kbps.last_valid(),
                |value| {
                    Message::Screen(message::Screen::Settings(
                        message::screen::settings::Message::TargetBitrateKbpsChanged(value),
                    ))
                }
            )
            .width(Length::Fill),
            text_input("Bitrate", draft.video.target_bitrate_kbps.text())
                .on_input(|value| {
                    Message::Screen(message::Screen::Settings(
                        message::screen::settings::Message::TargetBitrateKbpsInputChanged(value),
                    ))
                })
                .padding(8)
                .width(Length::Fixed(80.0))
                .style(theme::text_input_style),
            text("kbps").size(12).style(text::secondary)
        ]
        .spacing(15)
        .align_y(Alignment::Center)
        .width(Length::Fill);
        let mut bitrate = column![bitrate].spacing(5);
        if let Some(error) = draft.video.target_bitrate_kbps.error() {
            bitrate = bitrate.push(text(error).size(11).style(text::secondary));
        }
        let content = column![
             settings_section(
                lucide::arrow_right_left(),
                "Transcoding",
                column![
                    setting_row(
                        "Transcoder",
                        "Video codec and preferred encode path. Decoding uses GPU acceleration when available.",
                        pick_list(TranscodeType::ALL, Some(draft.video.transcoding_type), |val| {
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
                        pick_list(TargetResolution::ALL, Some(draft.video.target_resolution), |val| {
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
                        pick_list(Framerate::ALL, Some(draft.video.target_framerate), |val| {
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
                        bitrate,
                    ),
                ]
                .spacing(20),
            ),
        ]
        .spacing(25);

        scrollable(content).into()
    }
}
