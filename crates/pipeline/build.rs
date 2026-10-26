// The UnRAR code (compiled by the unrar-sys crate) calls the Windows registry
// and security functions, which live in advapi32; the crate does not link it.
fn main() {
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        println!("cargo:rustc-link-lib=advapi32");
    }
}
