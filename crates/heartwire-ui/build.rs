fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        .with_debug_info(true);
    slint_build::compile_with_config("../../ui/app.slint", config).expect("ui compiles");
}
