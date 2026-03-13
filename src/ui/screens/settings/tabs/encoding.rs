use iced::{
    Alignment, Element, Length,
    widget::{column, pick_list, row, scrollable, slider, text, text_input},
};
use iced_fonts::lucide;

use crate::{
    capture_providers::CaptureFramerate,
    config::Config,
    define_tab,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
    ui::{
        message::Message,
        screens::settings::{
            SettingsMessage,
            components::{setting_row, settings_section},
            tabs::SettingsTab,
        },
        theme,
    },
};

define_tab! {
    Encoding,
    icon: lucide::video(),
    view: |config| {
        let content = column![
             settings_section(
                lucide::video(),
                "Encoding",
                column![
                     setting_row(
                        "Codec",
                        "Video compression standard and hardware acceleration",
                        pick_list(FFmpegTranscodeType::ALL, Some(config.transcoding_type), |val| {
                            Message::Settings(SettingsMessage::TranscodingTypeChanged(val))
                        },)
                        .padding(8)
                        .width(Length::Fixed(200.0)),
                    ),
                     setting_row(
                        "Resolution",
                        "Maximum streaming resolution",
                        pick_list(TargetResolution::ALL, Some(config.target_resolution), |val| {
                            Message::Settings(SettingsMessage::TargetResolutionChanged(val))
                        },)
                        .padding(8)
                        .width(Length::Fixed(200.0)),
                    ),
                    setting_row(
                        "Framerate",
                        "Target frames per second",
                        pick_list(CaptureFramerate::ALL, Some(config.target_framerate), |val| {
                            Message::Settings(SettingsMessage::TargetFramerateChanged(val))
                        },)
                        .padding(8)
                        .width(Length::Fixed(200.0)),
                    ),
                    setting_row(
                        "Bitrate",
                        "Target bitrate for the video stream",
                        row![
                            slider(100_000..=10_000_000, config.target_bitrate, |val| {
                                Message::Settings(SettingsMessage::TargetBitrateChanged(val))
                            })
                            .width(Length::Fill),
                            text_input("Bitrate", &format!("{}", config.target_bitrate / 1000))
                                .on_input(|val| Message::Settings(
                                    SettingsMessage::TargetBitrateInputChanged(val)
                                ))
                                .padding(8)
                                .width(Length::Fixed(80.0))
                                .style(theme::text_input_style),
                            text("kbps").size(12).style(text::secondary)
                        ]
                        .spacing(15)
                        .align_y(Alignment::Center)
                        .width(Length::Fixed(400.0)),
                    ),
                ]
                .spacing(20),
            ),
        ]
        .spacing(25);

        scrollable(content).into()
    }
}
