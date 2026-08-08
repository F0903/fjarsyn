use std::{path::Path, process::Command};

pub(in crate::ui::shell) fn relaunch_current_executable() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not locate the Fjarsyn executable: {error}"))?;
    replacement_command(&executable)
        .spawn()
        .map_err(|error| format!("could not start a new Fjarsyn process: {error}"))?;
    Ok(())
}

fn replacement_command(executable: &Path) -> Command {
    // Do not forward command-line arguments: internal codec-worker arguments
    // must never leak into a replacement application process.
    Command::new(executable)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::replacement_command;

    #[test]
    fn replacement_process_starts_the_application_without_forwarded_arguments() {
        let command = replacement_command(Path::new("fjarsyn.exe"));

        assert_eq!(command.get_program(), "fjarsyn.exe");
        assert_eq!(command.get_args().count(), 0);
    }
}
