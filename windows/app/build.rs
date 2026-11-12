//! Compiles the Slint UI and, on a Windows build machine, puts the icon, version, and manifest
//! into the exe. The updater checks the exe's product name and version, and the manifest makes
//! the app DPI aware, so release exes must be built on Windows (the Windows workflow).
#![allow(clippy::print_stdout)] // cargo:rerun-if-changed lines are instructions to Cargo, not output.

fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    if let Err(error) = slint_build::compile_with_config("ui/panel.slint", config) {
        panic!("Slint UI: {error}");
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("../TypeThru/TypeThru.ico")
            .set_manifest_file("app.manifest")
            .set("ProductName", "TypeThru")
            .set("FileDescription", "TypeThru")
            .set("OriginalFilename", "TypeThru.exe")
            .set("LegalCopyright", "MIT License");
        if let Err(error) = resource.compile() {
            panic!("Windows resources: {error}");
        }
    }
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=../TypeThru/TypeThru.ico");
}
