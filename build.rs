fn main() {
    println!("cargo:rerun-if-env-changed=HR_OSC_VERSION");
    println!("cargo:rerun-if-env-changed=HR_OSC_CHANNEL");
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        .with_debug_info(true);
    slint_build::compile_with_config("ui/app.slint", config).expect("ui compiles");

    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Ok(version) = std::env::var("HR_OSC_VERSION") {
            res.set("FileVersion", &version);
            res.set("ProductVersion", &version);
        }
        res.set("ProductName", "hr-osc-rust");
        res.set("FileDescription", "hr-osc-rust");
        res.compile().expect("windows resources compile");
    }
}
