fn main() {
    println!("cargo:rustc-check-cfg=cfg(desktop)");
    println!("cargo:rustc-check-cfg=cfg(mobile)");
    #[cfg(feature = "app")]
    {
        let windows = tauri_build::WindowsAttributes::new()
            .append_rc_content(include_str!("windows/app-name.rc"));
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("could not prepare the Tauri application");
    }
}
