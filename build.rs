use std::io::Write;

fn main() -> std::io::Result<()> {
    writeln!(
        std::io::stdout().lock(),
        "cargo:rerun-if-env-changed=HEARTWIRE_VERSION"
    )?;
    writeln!(
        std::io::stdout().lock(),
        "cargo:rerun-if-env-changed=HEARTWIRE_CHANNEL"
    )?;
    #[cfg(windows)]
    {
        writeln!(
            std::io::stdout().lock(),
            "cargo:rerun-if-changed=assets/icon.ico"
        )?;
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
    Ok(())
}
