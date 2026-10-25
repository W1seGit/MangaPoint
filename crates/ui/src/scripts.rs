//! The page scripts both apps run, one copy each (`crates/ui/js`). Every script registers
//! `window.MP.<name>` with a `start(args)`; loading a script twice changes nothing. The desktop
//! app runs [`call`] in its web view; the website's build copies the files to `js/ui/` and loads
//! one by name the first time it is asked for (`apps/web/js/mp.js`, `run_script`).

/// Each script by the name it registers, and its source.
pub const SCRIPTS: [(&str, &str); 8] = [
    ("adder_scroll", include_str!("../js/adder-scroll.js")),
    ("fit_table", include_str!("../js/fit-table.js")),
    ("flip_grid", include_str!("../js/flip-grid.js")),
    ("export_split", include_str!("../js/export-split.js")),
    ("preview_editor", include_str!("../js/preview-editor.js")),
    ("tooltip", include_str!("../js/tooltip.js")),
    ("zoom_anchor", include_str!("../js/zoom-anchor.js")),
    ("zoom_scrub", include_str!("../js/zoom-scrub.js")),
];

/// The source of a script, by name.
pub fn source(name: &str) -> Option<&'static str> {
    SCRIPTS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// The code that loads a script (when it is not loaded yet) and starts it with `args` (JSON).
pub fn call(name: &str, args: &str) -> Option<String> {
    call_method(name, "start", args)
}

/// The code that loads a script and calls one of its functions with `args` (JSON, or any
/// JavaScript expression).
pub fn call_method(name: &str, method: &str, args: &str) -> Option<String> {
    let source = source(name)?;
    let args = if args.trim().is_empty() { "{}" } else { args };
    Some(format!("{source}\n;window.MP.{name}.{method}({args});"))
}

/// The file a script is kept in: `zoom_scrub` is `zoom-scrub.js`.
pub fn file_name(name: &str) -> String {
    format!("{}.js", name.replace('_', "-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_script_registers_the_name_it_is_called_by() {
        for (name, source) in SCRIPTS {
            assert!(source.contains(&format!("MP.{name} = ")), "{name} registers MP.{name}");
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("js").join(file_name(name));
            assert!(file.exists(), "{name} is kept in {}", file.display());
        }
    }

    #[test]
    fn a_call_loads_the_script_and_starts_it() {
        let code = call("zoom_scrub", "").unwrap();
        assert!(code.ends_with("window.MP.zoom_scrub.start({});"));
        assert!(call("nothing", "{}").is_none());
    }
}
