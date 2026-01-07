use std::{fmt::Display, time::Duration};

use serde::{Deserialize, Serialize};

use crate::define_enum_with_all;

macro_rules! define_framerates {
    ($($value:literal),* $(,)?) => {
        paste::paste! {
            define_enum_with_all! {
                #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Serialize, Deserialize)]
                pub enum CaptureFramerate {
                    $( [<FPS $value>] ),*
                }
            }

            impl CaptureFramerate {
                pub const fn to_hz(&self) -> f32 {
                    match self {
                        $(Self::[<FPS $value>] => $value as f32),*
                    }
                }

                pub fn to_frametime(&self) -> Duration {
                    Duration::from_secs_f32(1.0 / self.to_hz())
                }
            }

            impl Display for CaptureFramerate {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    match self {
                        $(Self::[<FPS $value>] => f.write_str(stringify!($value))),*
                    }
                }
            }
        }
    };
}

define_framerates! {
    5,
    24,
    30,
    60,
    120,
    144,
    200,
}
