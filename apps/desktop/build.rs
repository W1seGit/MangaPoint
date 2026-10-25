//! Embeds the mascot icon in the Windows executable so Explorer, the taskbar
//! and Alt+Tab show it. If the Windows resource compiler is missing, the
//! build still succeeds (the window icon is also set at run time).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winresource::WindowsResource::new();
        resources.set_icon("assets/icon.ico");
        if let Err(e) = resources.compile() {
            println!("cargo:warning=could not embed the executable icon: {e}");
        }
    }
}
