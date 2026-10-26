//! The figures drawn in Figma for the redesign (`crates/ui/assets/svg`), as they were drawn: the
//! icons of the dock and the corner bar, the small arrows, and the pixel scenery behind the
//! Search screen. Icons take their colour from the text (`currentColor`).

use dioxus::prelude::*;

/// The SVG called `name`, or `None` when there is no such figure.
pub fn svg(name: &str) -> Option<&'static str> {
    Some(match name {
        "nav-search" => include_str!("../../assets/svg/nav-search.svg"),
        "nav-library" => include_str!("../../assets/svg/nav-library.svg"),
        "nav-export" => include_str!("../../assets/svg/nav-export.svg"),
        "pulse" => include_str!("../../assets/svg/pulse.svg"),
        "gear" => include_str!("../../assets/svg/gear.svg"),
        "chevron" => include_str!("../../assets/svg/chevron.svg"),
        "search-button" => include_str!("../../assets/svg/search-button.svg"),
        "link" => include_str!("../../assets/svg/link.svg"),
        "reading" => include_str!("../../assets/svg/reading.svg"),
        "export-small" => include_str!("../../assets/svg/export-small.svg"),
        "download-small" => include_str!("../../assets/svg/download-small.svg"),
        "search-cloud-1" => include_str!("../../assets/svg/search-cloud-1.svg"),
        "search-cloud-2" => include_str!("../../assets/svg/search-cloud-2.svg"),
        "search-rock-1" => include_str!("../../assets/svg/search-rock-1.svg"),
        "search-rock-2" => include_str!("../../assets/svg/search-rock-2.svg"),
        _ => return None,
    })
}

/// A figure of the design, in a box that the style sheet sizes.
#[component]
pub fn Figure(name: &'static str, class: Option<&'static str>) -> Element {
    let body = svg(name).unwrap_or("");
    rsx! { span { class: "fig {class.unwrap_or_default()}", "aria-hidden": "true", dangerous_inner_html: "{body}" } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_figure_is_drawn() {
        for name in [
            "nav-search", "nav-library", "nav-export", "pulse", "gear", "chevron", "search-button", "link", "reading",
            "export-small", "download-small", "search-cloud-1", "search-cloud-2", "search-rock-1", "search-rock-2",
        ] {
            let svg = svg(name).unwrap_or_else(|| panic!("{name} is missing"));
            assert!(svg.starts_with("<svg") && svg.contains("viewBox"), "{name} is not an svg with a viewBox");
        }
    }
}
