fn main() {
    println!("cargo:rerun-if-env-changed=HEARTWIRE_VERSION");
    println!("cargo:rerun-if-env-changed=HEARTWIRE_CHANNEL");
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        .with_debug_info(true);
    slint_build::compile_with_config("ui/app.slint", config).expect("ui compiles");

    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Ok(version) = std::env::var("HEARTWIRE_VERSION") {
            res.set("FileVersion", &version);
            res.set("ProductVersion", &version);
        }
        res.set("ProductName", "Heartwire");
        res.set("FileDescription", "Heartwire");
        res.compile().expect("windows resources compile");
    }
}
