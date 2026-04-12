fn main() {
    if cfg!(target_os = "windows") {
        // FFmpeg's Media Foundation integration depends on COM interface IIDs
        // that are not reliably surfaced through Cargo metadata.
        println!("cargo:rustc-link-lib=mfuuid");
        println!("cargo:rustc-link-lib=strmiids");
    }
}
