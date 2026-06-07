fn main() {
    // In release, embed the requireAdministrator manifest for UAC elevation.
    // In debug/dev, use the default asInvoker manifest so `tauri dev` can run.
    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile == "release" {
        let attrs = tauri_build::Attributes::new()
            .windows_attributes(
                tauri_build::WindowsAttributes::new()
                    .app_manifest(include_str!("app.manifest")),
            );
        tauri_build::try_build(attrs).expect("failed to run tauri-build");
    } else {
        tauri_build::build();
    }
}
