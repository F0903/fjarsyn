use iced::{
    Alignment, Element, Length, mouse,
    widget::{Space, column, container, mouse_area, row},
    window,
};

use crate::ui::message::{self, Message};

pub(in crate::ui) fn resize_grid<'a>() -> Element<'a, Message> {
    let handle_size = 5.0;
    let corner_size = handle_size;

    let resize_handle =
        |direction: window::Direction, width: Length, height: Length| -> Element<'a, Message> {
            mouse_area(container(Space::new()).width(width).height(height))
                .on_press(Message::WindowControl(message::window::Control::Resize(direction)))
                .interaction(match direction {
                    window::Direction::North | window::Direction::South => {
                        mouse::Interaction::ResizingVertically
                    }
                    window::Direction::West | window::Direction::East => {
                        mouse::Interaction::ResizingHorizontally
                    }
                    window::Direction::NorthWest | window::Direction::SouthEast => {
                        mouse::Interaction::ResizingDiagonallyDown
                    }
                    window::Direction::NorthEast | window::Direction::SouthWest => {
                        mouse::Interaction::ResizingDiagonallyUp
                    }
                })
                .into()
        };

    column![
        row![
            resize_handle(window::Direction::NorthWest, corner_size.into(), corner_size.into()),
            resize_handle(window::Direction::North, Length::Fill, handle_size.into()),
            resize_handle(window::Direction::NorthEast, corner_size.into(), corner_size.into()),
        ]
        .spacing(0)
        .align_y(Alignment::Start),
        row![
            resize_handle(window::Direction::West, handle_size.into(), Length::Fill),
            Space::new().width(Length::Fill).height(Length::Fill),
            resize_handle(window::Direction::East, handle_size.into(), Length::Fill),
        ]
        .height(Length::Fill)
        .spacing(0),
        row![
            resize_handle(window::Direction::SouthWest, corner_size.into(), corner_size.into()),
            resize_handle(window::Direction::South, Length::Fill, handle_size.into()),
            resize_handle(window::Direction::SouthEast, corner_size.into(), corner_size.into()),
        ]
        .spacing(0)
        .align_y(Alignment::End),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .spacing(0)
    .into()
}
