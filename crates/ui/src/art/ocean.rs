//! The cat floating belly up on a calm sea at dusk, for the empty Search page.
//!
//! An original scene drawn in code on a coarse 80x40 grid, so every pixel is
//! big and visible. There is no frame: toward the edges the scene breaks up
//! into scattered pixels (decided pixel by pixel, not by a fade effect), until
//! only the page's own background is left, and the cat sits solid in the
//! middle. The motion is CSS (see `css/art.css`).

use dioxus::prelude::*;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The scene was drawn on 0..80 by 0..40 cells.
const W: i32 = 80;
const H: i32 = 40;
/// The wide picture for the Search page shows more of the same night around it:
/// the canvas reaches 80 cells to either side, 50 above and 20 below.
const X0: i32 = -80;
const Y0: i32 = -50;
const CW: i32 = 240;
const CH: i32 = 110;

/// How the edges of a picture break up into the page's black.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fade {
    /// Nothing is left out.
    None,
    /// Out from the middle in all directions (the small picture).
    Round,
    /// Away from the sides and the bottom (the wide picture); the top is whole.
    Wide,
}

/// Pixels collected from drawing, to be written out once the edges are broken up.
#[derive(Default)]
pub(super) struct Layer(Vec<(i32, i32, &'static str)>);

impl Layer {
    pub(super) fn px(&mut self, x: i32, y: i32, c: &'static str) {
        self.0.push((x, y, c));
    }

    pub(super) fn pixels(&mut self, points: &[(i32, i32)], c: &'static str) {
        for &(x, y) in points {
            self.px(x, y, c);
        }
    }

    pub(super) fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: &'static str) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.px(xx, yy, c);
            }
        }
    }

    pub(super) fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: &'static str) {
        for y in (cy - ry).floor() as i32..=(cy + ry).ceil() as i32 {
            for x in (cx - rx).floor() as i32..=(cx + rx).ceil() as i32 {
                let dx = (x as f32 + 0.5 - cx) / rx;
                let dy = (y as f32 + 0.5 - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    self.px(x, y, c);
                }
            }
        }
    }

    /// Dashes of one wave row, repeating every 8 pixels from off the left edge
    /// to off the right edge, so the row can drift by 8 and loop without a seam.
    fn wave_row(&mut self, y: i32, shift: i32, c: &'static str) {
        for k in -13..=22 {
            let base = k * 8 + shift;
            self.rect(base, y, 3, 1, c);
            self.rect(base + 5, y, 2, 1, c);
        }
    }
}

/// A repeatable pseudo-random number in 0..1 for a grid position.
fn noise(x: i32, y: i32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65_536.0
}

/// Does the pixel at (`x`, `y`) survive? Where the picture is whole, every
/// pixel does; toward its edge more and more are missing, in clumps and
/// singles, until nothing is left.
fn survives(fade: Fade, x: i32, y: i32) -> bool {
    let keep = match fade {
        Fade::None => 1.0,
        Fade::Round => {
            let dx = (x as f32 + 0.5 - W as f32 / 2.0) / (W as f32 / 2.0);
            let dy = (y as f32 + 0.5 - H as f32 / 2.0) / (H as f32 / 2.0);
            let d = (dx * dx + dy * dy).sqrt();
            if d <= 0.66 {
                1.0
            } else {
                1.0 - ((d - 0.66) / 0.42).clamp(0.0, 1.0)
            }
        }
        Fade::Wide => {
            // Cells from each edge: the first two are bare, then it thickens.
            let side = (x - X0).min(X0 + CW - 1 - x) as f32;
            let bottom = (Y0 + CH - 1 - y) as f32;
            let sides = ((side - 2.0) / 52.0).clamp(0.0, 1.0);
            let floor = ((bottom - 1.0) / 14.0).clamp(0.0, 1.0);
            sides.min(floor)
        }
    };
    // Mostly single pixels, with a few 2x2 clumps, so it looks scattered.
    let r = 0.6 * noise(x, y) + 0.4 * noise(x.div_euclid(2) + 91, y.div_euclid(2) + 17);
    r < keep
}

/// Write a layer as SVG: one rectangle per run of equal color in a row.
/// Later pixels cover earlier ones at the same place.
fn emit(layer: &Layer, fade: Fade) -> String {
    let mut rows: BTreeMap<i32, BTreeMap<i32, &str>> = BTreeMap::new();
    for &(x, y, c) in &layer.0 {
        if (Y0 - 10..Y0 + CH + 10).contains(&y) && (fade == Fade::None || survives(fade, x, y)) {
            rows.entry(y).or_default().insert(x, c);
        }
    }
    let mut out = String::new();
    for (y, row) in rows {
        let mut cells = row.into_iter().peekable();
        while let Some((x0, c)) = cells.next() {
            let mut end = x0;
            while let Some(&(x, next)) = cells.peek() {
                if x == end + 1 && next == c {
                    end = x;
                    cells.next();
                } else {
                    break;
                }
            }
            out.push_str(&format!(
                "<rect x=\"{x0}\" y=\"{y}\" width=\"{}\" height=\"1\" fill=\"{c}\"/>",
                end - x0 + 1
            ));
        }
    }
    out
}

/// A layer inside a group with a class (for the animation) and an opacity.
fn group(wide: bool, class: &str, opacity: f32, layer: &Layer) -> String {
    group_with(class, opacity, layer, if wide { Fade::Wide } else { Fade::Round })
}

/// Like [`group`], but every pixel is kept (no broken-up edges).
pub(super) fn group_plain(class: &str, opacity: f32, layer: &Layer) -> String {
    group_with(class, opacity, layer, Fade::None)
}

fn group_with(class: &str, opacity: f32, layer: &Layer, fade: Fade) -> String {
    let body = emit(layer, fade);
    if body.is_empty() {
        return String::new();
    }
    let class = if class.is_empty() {
        String::new()
    } else {
        format!(" class=\"{class}\"")
    };
    format!("<g{class} opacity=\"{opacity}\">{body}</g>")
}

const BODY: &str = "#a9a9a9";
const SHADE: &str = "#7d7d7d";
const LIGHT: &str = "#e8e8e8";
const BELLY: &str = "#f1ece2";
const PINK: &str = "#f0a3b3";
const DARK: &str = "#0a0a0a";

fn backdrop(wide: bool) -> String {
    let mut out = String::new();
    let mut bands = Layer::default();
    for (y0, y1, c) in [
        (0, 3, "#0b0d1c"),
        (4, 7, "#121832"),
        (8, 11, "#1b2242"),
        (12, 14, "#2a2e55"),
        (15, 16, "#443c66"),
        (17, 18, "#27425f"),
        (19, 22, "#203854"),
        (23, 27, "#192f48"),
        (28, 33, "#132639"),
        (34, 39, "#0e1c2f"),
    ] {
        bands.rect(X0 - 2, y0, CW + 4, y1 - y0 + 1, c);
    }
    out.push_str(&group(wide, "", 1.0, &bands));

    // The moon, with a glow around it.
    let mut glow = Layer::default();
    glow.ellipse(57.0, 8.0, 8.0, 8.0, "#f3ecd0");
    out.push_str(&group(wide, "", 0.08, &glow));
    let mut glow = Layer::default();
    glow.ellipse(57.0, 8.0, 5.5, 5.5, "#f3ecd0");
    out.push_str(&group(wide, "", 0.14, &glow));
    let mut moon = Layer::default();
    moon.ellipse(57.0, 8.0, 3.3, 3.3, "#f3ecd0");
    moon.pixels(&[(56, 7), (57, 7), (58, 10)], "#d9d1b2");
    out.push_str(&group(wide, "", 1.0, &moon));

    // Stars, in three groups that twinkle in turn.
    for (i, points) in [
        &[(6, 3), (19, 7), (33, 2), (47, 5), (70, 12)][..],
        &[(11, 9), (26, 4), (41, 8), (64, 2), (74, 6)][..],
        &[(4, 13), (22, 12), (37, 12), (51, 13), (68, 8)][..],
    ]
    .iter()
    .enumerate()
    {
        let mut stars = Layer::default();
        stars.pixels(points, "#dfe6ff");
        out.push_str(&group(wide, &format!("star s{i}"), 1.0, &stars));
    }

    // A cloud that drifts.
    let mut cloud = Layer::default();
    cloud.rect(9, 10, 8, 1, "#2c3358");
    cloud.rect(11, 9, 5, 1, "#2c3358");
    cloud.rect(13, 8, 2, 1, "#2c3358");
    out.push_str(&group(wide, "cloud", 1.0, &cloud));

    // The moon's path on the water.
    let mut glint = Layer::default();
    for (i, y) in (17..30).step_by(2).enumerate() {
        let w = 1 + (i as i32 % 3) * 2;
        glint.rect(57 - w / 2, y, w, 1, "#cdd5e6");
    }
    out.push_str(&group(wide, "glint", 0.4, &glint));

    // Far waves.
    let mut far = Layer::default();
    far.wave_row(18, 0, "#3a5f86");
    far.wave_row(21, 4, "#35597f");
    far.wave_row(25, 2, "#2f5176");
    out.push_str(&group(wide, "waves far", 1.0, &far));
    out
}

fn cat(wide: bool) -> String {
    let mut out = String::new();

    // The tail trails off to the right and curls up; two poses.
    for (class, spots, tip) in [
        ("tail-a", [(52, 25), (53, 24), (54, 23), (54, 22)], (55, 21)),
        ("tail-b", [(52, 25), (53, 24), (54, 24), (55, 23)], (56, 22)),
    ] {
        let mut tail = Layer::default();
        tail.pixels(&spots, SHADE);
        tail.px(tip.0, tip.1, "#4a4a4a");
        out.push_str(&group(wide, class, 1.0, &tail));
    }

    let mut s = Layer::default();
    // Body: a gray rim and a cream belly facing the sky.
    s.ellipse(41.0, 25.0, 11.0, 3.4, BODY);
    s.ellipse(41.0, 24.4, 9.0, 2.2, BELLY);
    s.pixels(&[(38, 24), (44, 25), (41, 23)], "#d9d2c5");
    s.pixels(&[(35, 28), (40, 28), (46, 28)], SHADE);

    // Hind legs, up in the air.
    s.ellipse(48.0, 24.0, 2.6, 2.1, BODY);
    s.rect(47, 20, 1, 3, BODY);
    s.rect(46, 18, 2, 2, LIGHT);
    s.px(46, 19, PINK);
    s.rect(50, 21, 1, 2, BODY);
    s.rect(49, 19, 2, 2, LIGHT);
    s.px(49, 20, PINK);

    // The head, seen upside down: eyes shut, the nose above them, ears toward
    // the water.
    s.ellipse(29.0, 25.0, 4.6, 3.9, BODY);
    s.ellipse(29.0, 23.4, 2.4, 1.5, LIGHT);
    s.rect(28, 22, 2, 1, PINK);
    s.pixels(&[(26, 25), (27, 26), (31, 26), (32, 25)], DARK);
    s.pixels(&[(28, 28), (29, 28), (30, 28)], SHADE);
    s.pixels(
        &[(25, 28), (26, 28), (25, 29), (33, 28), (32, 28), (33, 29)],
        BODY,
    );
    s.pixels(&[(26, 29), (32, 29)], PINK);
    out.push_str(&group(wide, "", 1.0, &s));

    // Front paws, up in the air; they twitch now and then.
    let mut paws = Layer::default();
    paws.rect(33, 21, 1, 3, BODY);
    paws.rect(32, 19, 2, 2, LIGHT);
    paws.px(32, 20, PINK);
    paws.rect(37, 20, 1, 4, BODY);
    paws.rect(36, 18, 2, 2, LIGHT);
    paws.px(36, 19, PINK);
    out.push_str(&group(wide, "paws", 1.0, &paws));

    // The water over the lower half of the cat.
    let mut under = Layer::default();
    under.rect(X0 - 2, 28, CW + 4, 2, "#192f48");
    under.rect(X0 - 2, 30, CW + 4, 6, "#132639");
    out.push_str(&group(wide, "", 0.8, &under));
    let mut foam = Layer::default();
    for (x, w) in [
        (21, 3),
        (26, 2),
        (31, 4),
        (37, 2),
        (42, 4),
        (48, 3),
        (53, 2),
        (58, 3),
    ] {
        foam.rect(x, 28, w, 1, "#a6c8e4");
    }
    out.push_str(&group(wide, "foam", 1.0, &foam));
    out
}

fn extras(wide: bool) -> String {
    let mut out = String::new();
    // Ripples spreading from the cat.
    for i in 0..2 {
        let mut ripple = Layer::default();
        ripple.rect(28, 32, 26, 1, "#7fa6c8");
        ripple.rect(24, 33, 4, 1, "#7fa6c8");
        ripple.rect(54, 33, 4, 1, "#7fa6c8");
        out.push_str(&group(wide, &format!("ripple r{i}"), 1.0, &ripple));
    }
    // A manga bobbing nearby.
    let mut book = Layer::default();
    book.rect(63, 28, 6, 3, "#6f86ad");
    book.rect(64, 28, 4, 2, "#e8e8e8");
    book.rect(63, 30, 6, 1, "#4a5d82");
    out.push_str(&group(wide, "book", 1.0, &book));
    // Sleepy "z"s drifting up from the cat.
    for (i, (x, y, big)) in [(23, 20, false), (19, 15, true)].into_iter().enumerate() {
        let rows: &[&str] = if big {
            &["###", "..#", ".#.", "###"]
        } else {
            &["##", ".#", "##"]
        };
        let mut z = Layer::default();
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.chars().enumerate() {
                if c == '#' {
                    z.px(x + dx as i32, y + dy as i32, "#e8e8e8");
                }
            }
        }
        out.push_str(&group(wide, &format!("zz z{i}"), 1.0, &z));
    }
    out
}

fn near_waves(wide: bool) -> String {
    let mut out = String::new();
    let mut mid = Layer::default();
    mid.wave_row(33, 1, "#2a4d73");
    mid.wave_row(35, 5, "#27476c");
    out.push_str(&group(wide, "waves mid", 1.0, &mid));
    let mut near = Layer::default();
    near.wave_row(37, 3, "#335a82");
    near.wave_row(39, 6, "#2f5379");
    out.push_str(&group(wide, "waves near", 1.0, &near));
    out
}

/// More of the night around the scene, for the wide picture: sky above that
/// darkens toward the top, more stars and clouds, and deeper water below.
fn wide_surroundings() -> String {
    let mut out = String::new();
    let mut bands = Layer::default();
    for (y0, y1, c) in [
        (-50, -39, "#06070f"),
        (-38, -27, "#080a16"),
        (-26, -15, "#0a0c1b"),
        (-14, -1, "#0b0d1c"),
        (40, 45, "#0c1928"),
        (46, 52, "#0a1623"),
        (53, 62, "#08121d"),
    ] {
        bands.rect(X0 - 2, y0, CW + 4, y1 - y0 + 1, c);
    }
    out.push_str(&group(true, "", 1.0, &bands));

    // Stars across the whole sky, in the same three groups that twinkle.
    for i in 0..3 {
        let mut stars = Layer::default();
        for n in 0..20 {
            let x = X0 + (noise(n * 7 + i * 31, 5) * CW as f32) as i32;
            let y = Y0 + (noise(n * 11 + 7, i * 13 + 2) * 62.0) as i32;
            // Not behind the name.
            if (-36..-12).contains(&y) && (-34..114).contains(&x) {
                continue;
            }
            stars.px(x, y, "#dfe6ff");
        }
        out.push_str(&group(true, &format!("star s{i}"), 1.0, &stars));
    }

    // More clouds that drift.
    for (cx, cy) in [(-40, -14), (98, -22), (-12, -6)] {
        let mut cloud = Layer::default();
        cloud.rect(cx, cy + 2, 9, 1, "#2c3358");
        cloud.rect(cx + 2, cy + 1, 5, 1, "#2c3358");
        cloud.rect(cx + 4, cy, 2, 1, "#2c3358");
        out.push_str(&group(true, "cloud", 1.0, &cloud));
    }

    // Waves in the deeper water.
    let mut deep = Layer::default();
    for (i, (y, c)) in [
        (41, "#2a4d73"),
        (43, "#27476c"),
        (46, "#244268"),
        (48, "#213c5d"),
        (51, "#1d3452"),
        (54, "#192d47"),
        (57, "#16263c"),
    ]
    .into_iter()
    .enumerate()
    {
        deep.wave_row(y, (i as i32 * 3) % 8, c);
    }
    out.push_str(&group(true, "waves mid", 1.0, &deep));
    out
}

/// "MANGA POINT" in big pixels across the top of the sky.
fn name() -> String {
    const SCALE: i32 = 2;
    let word = "MANGA POINT";
    let width = (word.chars().count() as i32 * 6 - 1) * SCALE;
    let left = X0 + CW / 2 - width / 2;
    let top = -31;
    let mut layer = Layer::default();
    // The shadow first, then the letters on top of it.
    for (shift, color) in [(SCALE, "#05070d"), (0, "#f2f0e6")] {
        for (n, c) in word.chars().enumerate() {
            if c == ' ' {
                continue;
            }
            for (row, bits) in super::placeholder::glyph(c).iter().enumerate() {
                for (col, bit) in bits.chars().enumerate() {
                    if bit == '1' {
                        layer.rect(
                            left + (n as i32 * 6 + col as i32) * SCALE + shift,
                            top + row as i32 * SCALE + shift,
                            SCALE,
                            SCALE,
                            color,
                        );
                    }
                }
            }
        }
    }
    group_plain("", 1.0, &layer)
}

fn hero_markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        format!(
            "{}{}<g class=\"floater\">{}</g>{}{}{}",
            wide_surroundings(),
            backdrop(true),
            cat(true),
            extras(true),
            near_waves(true),
            name()
        )
    })
}

fn markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        format!(
            "{}<g class=\"floater\">{}</g>{}{}",
            backdrop(false),
            cat(false),
            extras(false),
            near_waves(false)
        )
    })
}

/// The cat floating on its back, filling most of the empty Search page.
#[component]
pub fn OceanCat() -> Element {
    rsx! {
        svg {
            class: "ocean",
            view_box: "0 0 {W} {H}",
            role: "img",
            "aria-label": "A cat floating on its back in a calm sea at dusk",
            dangerous_inner_html: "{markup()}",
        }
    }
}

/// The wide picture of the same night, for the top of the Search page: the cat
/// as big as before, with the sky and the sea reaching out around it, the name
/// across the top, and the bottom breaking up into black.
#[component]
pub fn OceanHero() -> Element {
    rsx! {
        svg {
            class: "ocean hero",
            view_box: "{X0} {Y0} {CW} {CH}",
            role: "img",
            "aria-label": "MangaPoint: a cat floating on its back in a calm sea at night",
            dangerous_inner_html: "{hero_markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects() -> Vec<(i32, i32, i32)> {
        markup()
            .split("<rect ")
            .skip(1)
            .map(|part| {
                let num = |key: &str| -> i32 {
                    let rest = &part[part.find(key).unwrap() + key.len()..];
                    rest[..rest.find('"').unwrap()].parse().unwrap()
                };
                (num("x=\""), num("y=\""), num("width=\""))
            })
            .collect()
    }

    #[test]
    fn nothing_is_drawn_far_outside_the_canvas() {
        for (x, y, w) in rects() {
            assert!(
                x >= -10 && x + w <= W + 10 && (0..H).contains(&y),
                "{x},{y},{w}"
            );
        }
    }

    #[test]
    fn the_scene_has_its_moving_parts() {
        let m = markup();
        for class in [
            "floater",
            "tail-a",
            "tail-b",
            "paws",
            "foam",
            "ripple r1",
            "star s2",
            "waves far",
            "waves near",
            "zz z1",
            "book",
            "cloud",
        ] {
            assert!(m.contains(&format!("class=\"{class}\"")), "{class}");
        }
    }

    #[test]
    fn the_middle_is_solid_and_the_edges_break_up() {
        let covered = |x0: i32, x1: i32, y0: i32, y1: i32| -> usize {
            (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .filter(|&(x, y)| survives(Fade::Round, x, y))
                .count()
        };
        // The cat's area: nothing missing.
        assert_eq!(covered(24, 56, 17, 30), 32 * 13);
        // The outer ring is mostly gone, the corners entirely.
        let ring = covered(0, 6, 0, 40) + covered(74, 80, 0, 40);
        assert!(ring < 6 * 40 * 2 / 4, "{ring}");
        assert_eq!(covered(0, 4, 0, 4), 0);
        assert_eq!(covered(76, 80, 36, 40), 0);
    }

    #[test]
    fn the_cats_belly_and_face_are_all_there() {
        let m = markup();
        assert!(m.contains(BELLY) && m.contains(PINK) && m.contains(DARK));
    }

    #[test]
    fn the_wide_picture_is_whole_at_the_top_and_breaks_up_at_the_sides_and_bottom() {
        let covered = |x0: i32, x1: i32, y0: i32, y1: i32| -> usize {
            (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .filter(|&(x, y)| survives(Fade::Wide, x, y))
                .count()
        };
        // The cat's area and the sky above it: nothing missing.
        assert_eq!(covered(24, 56, 17, 30), 32 * 13);
        assert_eq!(covered(-10, 90, -50, -20), 100 * 30);
        // The edges are bare, and the last rows before the bottom are thin.
        assert_eq!(covered(X0, X0 + 2, 0, 40), 0);
        assert_eq!(covered(0, 80, Y0 + CH - 1, Y0 + CH), 0);
        // Whole until late, then thinning toward the last row.
        assert!(covered(0, 80, Y0 + CH - 6, Y0 + CH - 2) < covered(0, 80, Y0 + CH - 22, Y0 + CH - 18));
    }

    #[test]
    fn the_wide_picture_has_the_name_and_the_same_moving_parts() {
        let m = hero_markup();
        for class in ["floater", "tail-a", "paws", "waves near", "waves mid", "star s2", "cloud", "zz z1"] {
            assert!(m.contains(&format!("class=\"{class}\"")), "{class}");
        }
        // The name is plain cream letters with a shadow.
        assert!(m.contains("#f2f0e6") && m.contains("#05070d"));
        for (x, y, w) in m.split("<rect ").skip(1).map(|part| {
            let num = |key: &str| -> i32 {
                let rest = &part[part.find(key).unwrap() + key.len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            };
            (num("x=\""), num("y=\""), num("width=\""))
        }) {
            assert!(
                x >= X0 - 12 && x + w <= X0 + CW + 12 && (Y0..Y0 + CH + 2).contains(&y),
                "{x},{y},{w}"
            );
        }
    }
}
