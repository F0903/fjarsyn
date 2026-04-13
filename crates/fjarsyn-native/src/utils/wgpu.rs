use fjarsyn_core::config::PowerPref;

pub fn apply_wgpu_power_pref(power_pref: PowerPref) {
    unsafe {
        match power_pref {
            PowerPref::Low => std::env::set_var("WGPU_POWER_PREF", "low"),
            PowerPref::Max => std::env::set_var("WGPU_POWER_PREF", "high"),
        }
    }
}

pub fn configure_default_wgpu_backend() {
    #[cfg(target_os = "windows")]
    {
        configure_windows_default_wgpu_backend();
    }
}

#[cfg(target_os = "windows")]
fn configure_windows_default_wgpu_backend() {
    if std::env::var("WGPU_BACKEND").is_err() {
        unsafe {
            std::env::set_var("WGPU_BACKEND", "dx12");
        }
    }
}
