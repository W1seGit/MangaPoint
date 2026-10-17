//! Lucide icons (https://lucide.dev), used under the ISC license in
//! LUCIDE-LICENSE.txt.
//!
//! Each entry is the inner markup of the icon's SVG from the Lucide
//! repository. The 24x24 view box and the stroke styling are applied by
//! widgets::Icon.

pub fn body(name: &str) -> Option<&'static str> {
    match name {
        "search" => Some(r#"<path d="m21 21-4.34-4.34" /> <circle cx="11" cy="11" r="8" />"#),
        "download" => Some(
            r#"<path d="M12 15V3" /> <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /> <path d="m7 10 5 5 5-5" />"#,
        ),
        "settings" => Some(
            r#"<path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915" /> <circle cx="12" cy="12" r="3" />"#,
        ),
        "layers" => Some(
            r#"<path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z" /> <path d="M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12" /> <path d="M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17" />"#,
        ),
        "clock" => Some(r#"<circle cx="12" cy="12" r="10" /> <path d="M12 6v6l4 2" />"#),
        "check" => Some(r#"<path d="M20 6 9 17l-5-5" />"#),
        "x" => Some(r#"<path d="M18 6 6 18" /> <path d="m6 6 12 12" />"#),
        "chevron-up" => Some(r#"<path d="m18 15-6-6-6 6" />"#),
        "chevron-down" => Some(r#"<path d="m6 9 6 6 6-6" />"#),
        "chevrons-up-down" => Some(r#"<path d="m7 15 5 5 5-5" /> <path d="m7 9 5-5 5 5" />"#),
        "arrow-left" => Some(r#"<path d="m12 19-7-7 7-7" /> <path d="M19 12H5" />"#),
        "refresh-cw" => Some(
            r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" /> <path d="M21 3v5h-5" /> <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" /> <path d="M8 16H3v5" />"#,
        ),
        "trash" => Some(
            r#"<path d="M10 11v6" /> <path d="M14 11v6" /> <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /> <path d="M3 6h18" /> <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />"#,
        ),
        "plus" => Some(r#"<path d="M5 12h14" /> <path d="M12 5v14" />"#),
        "folder-open" => Some(
            r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" />"#,
        ),
        "hard-drive" => Some(
            r#"<path d="M10 16h.01" /> <path d="M2.212 11.577a2 2 0 0 0-.212.896V18a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-5.527a2 2 0 0 0-.212-.896L18.55 5.11A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" /> <path d="M21.946 12.013H2.054" /> <path d="M6 16h.01" />"#,
        ),
        "wifi" => Some(
            r#"<path d="M12 20h.01" /> <path d="M2 8.82a15 15 0 0 1 20 0" /> <path d="M5 12.859a10 10 0 0 1 14 0" /> <path d="M8.5 16.429a5 5 0 0 1 7 0" />"#,
        ),
        "triangle-alert" => Some(
            r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" /> <path d="M12 9v4" /> <path d="M12 17h.01" />"#,
        ),
        "info" => Some(
            r#"<circle cx="12" cy="12" r="10" /> <path d="M12 16v-4" /> <path d="M12 8h.01" />"#,
        ),
        "book-open" => Some(
            r#"<path d="M12 5v16" /> <path d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z" />"#,
        ),
        "loader-circle" => Some(r#"<path d="M21 12a9 9 0 1 1-6.219-8.56" />"#),
        "file-down" => Some(
            r#"<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" /> <path d="M14 2v5a1 1 0 0 0 1 1h5" /> <path d="M12 18v-6" /> <path d="m9 15 3 3 3-3" />"#,
        ),
        "circle-check" => {
            Some(r#"<circle cx="12" cy="12" r="10" /> <path d="m16 9-5.5 5.5L8 12" />"#)
        }
        "circle-x" => Some(
            r#"<circle cx="12" cy="12" r="10" /> <path d="m15 9-6 6" /> <path d="m9 9 6 6" />"#,
        ),
        "sliders-horizontal" => Some(
            r#"<path d="M10 5H3" /> <path d="M12 19H3" /> <path d="M14 3v4" /> <path d="M16 17v4" /> <path d="M21 12h-9" /> <path d="M21 19h-5" /> <path d="M21 5h-7" /> <path d="M8 10v4" /> <path d="M8 12H3" />"#,
        ),
        "globe" => Some(
            r#"<circle cx="12" cy="12" r="10" /> <path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" /> <path d="M2 12h20" />"#,
        ),
        "funnel" => Some(
            r#"<path d="M10 20a1 1 0 0 0 .553.895l2 1A1 1 0 0 0 14 21v-7a2 2 0 0 1 .517-1.341L21.74 4.67A1 1 0 0 0 21 3H3a1 1 0 0 0-.742 1.67l7.225 7.989A2 2 0 0 1 10 14z" />"#,
        ),
        "circle-alert" => Some(
            r#"<circle cx="12" cy="12" r="10" /> <line x1="12" x2="12" y1="8" y2="12" /> <line x1="12" x2="12.01" y1="16" y2="16" />"#,
        ),
        "command" => Some(
            r#"<path d="M15 6v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3" />"#,
        ),
        "save" => Some(
            r#"<path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" /> <path d="M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7" /> <path d="M7 3v4a1 1 0 0 0 1 1h7" />"#,
        ),
        "zoom-in" => Some(
            r#"<circle cx="11" cy="11" r="8" /> <line x1="21" x2="16.65" y1="21" y2="16.65" /> <line x1="11" x2="11" y1="8" y2="14" /> <line x1="8" x2="14" y1="11" y2="11" />"#,
        ),
        "move-horizontal" => {
            Some(r#"<path d="m18 8 4 4-4 4" /> <path d="M2 12h20" /> <path d="m6 8-4 4 4 4" />"#)
        }
        "external-link" => Some(
            r#"<path d="M15 3h6v6" /> <path d="M10 14 21 3" /> <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />"#,
        ),
        "rotate-ccw" => Some(
            r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /> <path d="M3 3v5h5" />"#,
        ),
        "arrow-down-up" => Some(
            r#"<path d="m3 16 4 4 4-4" /> <path d="M7 20V4" /> <path d="m21 8-4-4-4 4" /> <path d="M17 4v16" />"#,
        ),
        "list-filter" => Some(r#"<path d="M2 5h20" /> <path d="M6 12h12" /> <path d="M9 19h6" />"#),
        "eye" => Some(
            r#"<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" /> <circle cx="12" cy="12" r="3" />"#,
        ),
        "list-checks" => Some(
            r#"<path d="M13 5h8" /> <path d="M13 12h8" /> <path d="M13 19h8" /> <path d="m3 17 2 2 4-4" /> <path d="m3 7 2 2 4-4" />"#,
        ),
        "image" => Some(
            r#"<rect width="18" height="18" x="3" y="3" rx="2" ry="2" /> <circle cx="9" cy="9" r="2" /> <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />"#,
        ),
        "book-open-check" => Some(
            r#"<path d="M12 5v16" /> <path d="m16 12 2 2 4-4" /> <path d="M22 6V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2h4.001A2 2 0 0022 17v-1.344" />"#,
        ),
        "pencil" => Some(
            r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" /> <path d="m15 5 4 4" />"#,
        ),
        "panel-right-close" => Some(
            r#"<rect width="18" height="18" x="3" y="3" rx="2" /> <path d="M15 3v18" /> <path d="m8 9 3 3-3 3" />"#,
        ),
        "panel-right-open" => Some(
            r#"<rect width="18" height="18" x="3" y="3" rx="2" /> <path d="M15 3v18" /> <path d="m10 15-3-3 3-3" />"#,
        ),
        "crop" => {
            Some(r#"<path d="M6 2v14a2 2 0 0 0 2 2h14" /> <path d="M18 22V8a2 2 0 0 0-2-2H2" />"#)
        }
        "bookmark-plus" => Some(
            r#"<path d="M12 7v6" /> <path d="M15 10H9" /> <path d="M17 3a2 2 0 0 1 2 2v15a1 1 0 0 1-1.496.868l-4.512-2.578a2 2 0 0 0-1.984 0l-4.512 2.578A1 1 0 0 1 5 20V5a2 2 0 0 1 2-2z" />"#,
        ),
        "bookmark-check" => Some(
            r#"<path d="M17 3a2 2 0 0 1 2 2v15a1 1 0 0 1-1.496.868l-4.512-2.578a2 2 0 0 0-1.984 0l-4.512 2.578A1 1 0 0 1 5 20V5a2 2 0 0 1 2-2z" /> <path d="m9 10 2 2 4-4" />"#,
        ),
        "pause" => Some(
            r#"<rect x="14" y="3" width="5" height="18" rx="1" /> <rect x="5" y="3" width="5" height="18" rx="1" />"#,
        ),
        "play" => Some(
            r#"<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z" />"#,
        ),
        "arrow-up-to-line" => {
            Some(r#"<path d="M5 3h14" /> <path d="m18 13-6-6-6 6" /> <path d="M12 7v14" />"#)
        }
        "rotate-cw" => Some(
            r#"<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" /> <path d="M21 3v5h-5" />"#,
        ),
        "ellipsis" => Some(
            r#"<circle cx="12" cy="12" r="1" /> <circle cx="19" cy="12" r="1" /> <circle cx="5" cy="12" r="1" />"#,
        ),
        "chevron-right" => Some(r#"<path d="m9 18 6-6-6-6" />"#),
        "clock-3" => Some(r#"<circle cx="12" cy="12" r="10" /> <path d="M12 6v6h4" />"#),
        "link" => Some(
            r#"<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" /> <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />"#,
        ),
        "unlink" => Some(
            r#"<path d="m18.84 12.25 1.72-1.71h-.02a5.004 5.004 0 0 0-.12-7.07 5.006 5.006 0 0 0-6.95 0l-1.72 1.71" /> <path d="m5.17 11.75-1.71 1.71a5.004 5.004 0 0 0 .12 7.07 5.006 5.006 0 0 0 6.95 0l1.71-1.71" /> <line x1="8" x2="8" y1="2" y2="5" /> <line x1="2" x2="5" y1="8" y2="8" /> <line x1="16" x2="16" y1="19" y2="22" /> <line x1="19" x2="22" y1="16" y2="16" />"#,
        ),
        "lock" => Some(
            r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2" /> <path d="M7 11V7a5 5 0 0 1 10 0v4" />"#,
        ),
        "lock-open" => Some(
            r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2" /> <path d="M7 11V7a5 5 0 0 1 9.9-1" />"#,
        ),
        "circle-help" => Some(
            r#"<circle cx="12" cy="12" r="10" /> <path d="M9.09 9a3 3 0 0 1 5.82 1c0 2-3 3-3 3" /> <path d="M12 17h.01" />"#,
        ),
        _ => None,
    }
}
