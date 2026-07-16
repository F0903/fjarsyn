use iced::Task;

use crate::ui::{
    message::{ConfigMessage, Message},
    shell::Fjarsyn,
};

pub fn handle_config_msg(app: &mut Fjarsyn, message: ConfigMessage) -> Task<Message> {
    match message {
        ConfigMessage::SaveRequested(mut config) => {
            // Identity belongs to the running authenticated session and cannot
            // be replaced by an unrelated settings draft.
            config.identity = app.ctx.config.identity.clone();
            match config.save() {
                Ok(()) => {
                    let network_restart_required =
                        app.runtime.application.as_ref().is_some_and(|runtime| {
                            runtime.active_config.network.max_depacket_latency
                                != config.network.max_depacket_latency
                        });
                    if let Some(runtime) = app.runtime.application.as_ref() {
                        runtime.update_media_config(&config);
                    }
                    app.ctx.config = config;
                    if network_restart_required {
                        app.ctx.notify_success(
                            "Settings saved. Network changes apply after restarting Fjarsyn.",
                        );
                    } else {
                        app.ctx.notify_success("Settings saved.");
                    }
                }
                Err(error) => app.ctx.notify_error(format!("Failed to save settings: {error}")),
            }
        }
    }
    Task::none()
}
