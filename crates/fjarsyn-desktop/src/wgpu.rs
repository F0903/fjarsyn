//! Process-wide wgpu backend and power-preference configuration.

use fjarsyn_engine::config::PowerPref;

pub(super) fn apply_wgpu_power_pref(power_pref: PowerPref) {
    // SAFETY: application startup sets process environment before tracing,
    // Iced, or any application-owned worker threads are started.
    unsafe {
        match power_pref {
            PowerPref::Low => std::env::set_var("WGPU_POWER_PREF", "low"),
            PowerPref::Max => std::env::set_var("WGPU_POWER_PREF", "high"),
        }
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
