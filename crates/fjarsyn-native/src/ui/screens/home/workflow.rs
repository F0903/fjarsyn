use super::{HomeMessage, HomeScreen};

// Home state changes are simple enough to stay as pure mutations.
pub(crate) fn execute_home_message(screen: &mut HomeScreen, message: HomeMessage) {
    match message {
        HomeMessage::TargetAddressChanged(value) => {
            screen.manual_target_address = value;
        }
    }
}
