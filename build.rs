use std::env;

use winres::{VersionInfo, WindowsResource};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn normalized_version_components(version: &str) -> [u16; 4] {
    let numeric = version.split(|c| c == '-' || c == '+').next().unwrap_or(version);
    let mut parts = [0u16; 4];
    for (idx, piece) in numeric.split('.').take(4).enumerate() {
        if let Ok(value) = piece.parse::<u16>() {
            parts[idx] = value;
        }
    }
    parts
}

fn components_to_u64(parts: [u16; 4]) -> u64 {
    ((parts[0] as u64) << 48)
        | ((parts[1] as u64) << 32)
        | ((parts[2] as u64) << 16)
        | parts[3] as u64
}

fn components_to_string(parts: [u16; 4]) -> String {
    format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3])
}

fn set_windows_metadata() -> Result<()> {
    let pkg_name = env::var("CARGO_PKG_NAME")?;
    let pkg_version = env::var("CARGO_PKG_VERSION")?;
    let version_components = normalized_version_components(&pkg_version);
    let version_u64 = components_to_u64(version_components);
    let version_string = components_to_string(version_components);
    let internal_name = format!("{}.exe", pkg_name);

    let mut res = WindowsResource::new();
    res.set("InternalName", &internal_name)
        .set("OriginalFilename", &internal_name)
        .set("ProductName", &pkg_name)
        .set("FileDescription", &pkg_name) // For some reason this is shown as the app name in Task Manager. So we just use the package name as the "file description".
        .set("ProductVersion", &version_string)
        .set("FileVersion", &version_string)
        .set_version_info(VersionInfo::PRODUCTVERSION, version_u64)
        .set_version_info(VersionInfo::FILEVERSION, version_u64);
    res.compile()?;
    Ok(())
}

fn main() {
    if cfg!(target_os = "windows") {
        set_windows_metadata().expect("failed to embed Windows resources");
    }
}
