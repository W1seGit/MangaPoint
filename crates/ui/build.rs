//! Makes `mangapoint_ui::CSS`: every shared style sheet in `css/`, in the order `css/ORDER` lists
//! them, so the desktop app and the website (`tools/build-web.*`) load them the same way.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let dir = Path::new(&root).join("css");
    println!("cargo:rerun-if-changed=css");
    let order = std::fs::read_to_string(dir.join("ORDER")).expect("css/ORDER lists the style sheets");
    let mut out = String::from("/// Every shared style sheet (`crates/ui/css`), in the order of `css/ORDER`.\npub const CSS: &str = concat!(\n");
    for name in order.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let path = dir.join(name);
        assert!(path.is_file(), "css/ORDER lists {name}, which is not in css/");
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(out, "    include_str!({:?}),", path.display().to_string()).unwrap();
    }
    out.push_str(");\n");
    let dest = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("css.rs");
    std::fs::write(dest, out).expect("the generated CSS list is written");
}
