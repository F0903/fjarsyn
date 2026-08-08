use fjarsyn_engine::screen_share;
use iced::Task;

use crate::ui::{
    message::{self, Message},
    shell::Fjarsyn,
};

pub(in crate::ui::shell) fn handle_config_msg(
    app: &mut Fjarsyn,
    message: message::Config,
) -> Task<Message> {
    match message {
        message::Config::SaveRequested(mut config) => {
            // Identity belongs to the running authenticated session and cannot
            // be replaced by an unrelated settings draft.
            config.identity = app.state.config.identity.clone();
            match config.save() {
                Ok(()) => {
                    let network_restart_required =
                        app.runtime.application.as_ref().is_some_and(|runtime| {
                            runtime.active_config().network.max_depacket_latency
                                != config.network.max_depacket_latency
                        });
                    if let Some(runtime) = app.runtime.application.as_ref() {
                        runtime.screen_share().update_config(screen_share::Config::from(&config));
                    }
                    app.state.config = config;
                    if network_restart_required {
                        app.state.notify_success(
                            "Settings saved. Network changes apply after restarting Fjarsyn.",
                        );
                    } else {
                        app.state.notify_success("Settings saved.");
                    }
                }
                Err(error) => app.state.notify_error(format!("Failed to save settings: {error}")),
            }
        }
    }
    Task::none()
}
