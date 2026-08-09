//! Process-wide wgpu backend and power-preference configuration.

use crate::settings::PowerPreference;

pub(super) fn apply_wgpu_power_preference(power_preference: PowerPreference) {
    // SAFETY: application startup sets process environment before tracing,
    // Iced, or any application-owned worker threads are started.
    unsafe {
        std::env::set_var("WGPU_POWER_PREF", wgpu_power_preference(power_preference));
    }
}

fn wgpu_power_preference(power_preference: PowerPreference) -> &'static str {
    match power_preference {
        PowerPreference::LowPower => "low",
        PowerPreference::HighPerformance => "high",
    }
}

pub(super) fn configure_default_wgpu_backend() {
    #[cfg(target_os = "windows")]
    {
        configure_windows_default_wgpu_backend();
    }
}

#[cfg(target_os = "windows")]
fn configure_windows_default_wgpu_backend() {
    if std::env::var("WGPU_BACKEND").is_err() {
        // SAFETY: application startup sets process environment before tracing,
        // Iced, or any application-owned worker threads are started.
        unsafe {
            std::env::set_var("WGPU_BACKEND", "dx12");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_power_preferences_map_to_wgpu_values() {
        assert_eq!(wgpu_power_preference(PowerPreference::LowPower), "low");
        assert_eq!(wgpu_power_preference(PowerPreference::HighPerformance), "high");
    }
}
