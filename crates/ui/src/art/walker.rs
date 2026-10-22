//! A small pixel-art cat walking along a city sidewalk at night, seen from the
//! side, with a skyline, buildings and street lamps drifting past behind it.
//! The whole picture is one SVG built once; the browser plays it from CSS
//! keyframes (see `style.css`). Nothing glides: the cat changes pose six times
//! in a step cycle, and the street moves in whole pixels in time with its feet.

use dioxus::prelude::*;
use std::sync::OnceLock;

/// The size of the picture, in its own big pixels.
const VIEW_W: i32 = 200;
const VIEW_H: i32 = 62;
/// The row the sidewalk starts at (and the cat's feet stand on).
const GROUND_Y: i32 = 52;

const W: usize = 22;
const H: usize = 13;
type Grid = [[u8; W]; H];

const CLEAR: u8 = b'.';
const BODY: u8 = b'g';
const SHADE: u8 = b's';
const LIGHT: u8 = b'l';
const PINK: u8 = b'p';
const EYE: u8 = b'e';

/// Poses in one step cycle.
const FRAMES: usize = 6;

fn color(c: u8) -> &'static str {
    match c {
        BODY => "#b4b4b4",
        SHADE => "#707070",
        LIGHT => "#e8e8e8",
        PINK => "#f0a3b3",
        EYE => "#050505",
        _ => "none",
    }
}

fn put(g: &mut Grid, x: usize, y: usize, c: u8) {
    if x < W && y < H {
        g[y][x] = c;
    }
}

fn fill(g: &mut Grid, x0: usize, y0: usize, x1: usize, y1: usize, c: u8) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            put(g, x, y, c);
        }
    }
}

/// One leg at `phase` of its stride (0..6) with its hip at `hip`: planted and
/// pushing back through 0, 1, 2; then lifted and swinging forward through 3, 4, 5.
fn leg(g: &mut Grid, hip: usize, phase: usize, dy: usize, c: u8) {
    let (x, end) = match phase % FRAMES {
        0 => (hip + 1, 12),
        1 => (hip, 12),
        2 => (hip - 1, 12),
        3 => (hip - 1, 11),
        4 => (hip, 10),
        _ => (hip + 1, 11),
    };
    fill(g, x, 9 + dy, x + 1, end, c);
    fill(g, x, end, x + 1, end, LIGHT);
}

/// One pose of the walk. The cat faces right and walks on a diagonal gait: a
/// front leg and the opposite rear leg move together.
fn frame(f: usize) -> Grid {
    let mut g = [[CLEAR; W]; H];
    // The body dips as the feet land, and rises while the legs pass under it.
    let dy = usize::from(f.is_multiple_of(3));

    // The tail sways up, across and down.
    const TAILS: [[(usize, usize); 4]; FRAMES] = [
        [(2, 4), (1, 3), (1, 2), (0, 1)],
        [(2, 4), (1, 3), (0, 3), (0, 2)],
        [(2, 4), (1, 4), (0, 4), (0, 3)],
        [(2, 5), (1, 5), (0, 5), (0, 4)],
        [(2, 4), (1, 4), (0, 4), (0, 3)],
        [(2, 4), (1, 3), (0, 3), (0, 2)],
    ];
    for (x, y) in TAILS[f] {
        put(&mut g, x, y + dy, SHADE);
    }

    // Far legs behind (darker), near legs in front.
    leg(&mut g, 13, f + 3, dy, SHADE); // front far
    leg(&mut g, 3, f, dy, SHADE); // rear far
    leg(&mut g, 13, f, dy, BODY); // front near
    leg(&mut g, 3, f + 3, dy, BODY); // rear near

    // Body, with stripes along the back and a light belly.
    fill(&mut g, 3, 3 + dy, 14, 8 + dy, BODY);
    put(&mut g, 3, 3 + dy, CLEAR);
    put(&mut g, 3, 8 + dy, CLEAR);
    for x in [6, 9, 12] {
        put(&mut g, x, 3 + dy, SHADE);
    }
    fill(&mut g, 6, 8 + dy, 12, 8 + dy, LIGHT);

    // Head, ears, eye and muzzle.
    fill(&mut g, 13, 1 + dy, 18, 6 + dy, BODY);
    fill(&mut g, 13, dy, 14, dy, BODY);
    fill(&mut g, 17, dy, 18, dy, BODY);
    put(&mut g, 14, 1 + dy, PINK);
    put(&mut g, 17, 1 + dy, PINK);
    put(&mut g, 17, 3 + dy, EYE);
    fill(&mut g, 19, 4 + dy, 19, 5 + dy, LIGHT);
    put(&mut g, 19, 4 + dy, PINK);
    put(&mut g, 18, 5 + dy, LIGHT);
    g
}

/// A grid as SVG rectangles, one per run of the same colour in a row.
fn rects(g: &Grid) -> String {
    let mut out = String::new();
    for (y, row) in g.iter().enumerate() {
        let mut x = 0;
        while x < W {
            let c = row[x];
            if c == CLEAR {
                x += 1;
                continue;
            }
            let start = x;
            while x < W && row[x] == c {
                x += 1;
            }
            out.push_str(&format!(
                "<rect x=\"{start}\" y=\"{y}\" width=\"{}\" height=\"1\" fill=\"{}\"/>",
                x - start,
                color(c)
            ));
        }
    }
    out
}

fn rect(x: i32, y: i32, w: i32, h: i32, fill: &str) -> String {
    format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>")
}

/// `count` copies of `unit`, `period` apart.
fn repeat(unit: &str, period: i32, count: i32) -> String {
    (0..count)
        .map(|i| format!("<g transform=\"translate({} 0)\">{unit}</g>", i * period))
        .collect()
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

/// Does the picture's pixel at (`x`, `y`) show? Whole in the middle; toward
/// the sides and the bottom more and more are missing, in singles and a few
/// 2x2 clumps, until nothing is left. Decided pixel by pixel, like the
/// picture on the search page.
fn shows(x: i32, y: i32) -> bool {
    let side = x.min(VIEW_W - 1 - x) as f32;
    let bottom = (VIEW_H - 1 - y) as f32;
    let top = y as f32;
    let keep = ((side - 2.0) / 40.0)
        .clamp(0.0, 1.0)
        .min(((bottom - 1.0) / 8.0).clamp(0.0, 1.0))
        .min(((top - 1.0) / 9.0).clamp(0.0, 1.0));
    let r = 0.6 * noise(x, y) + 0.4 * noise(x.div_euclid(2) + 91, y.div_euclid(2) + 17);
    r < keep
}

/// Page-black squares laid over the scene wherever a pixel does not show, so
/// the scene keeps moving underneath while its edges stay broken up.
fn edge_cover() -> String {
    // A black frame one pixel outside the picture: scaled to the window the
    // outer row would otherwise show through as a thin line.
    let mut out = String::new();
    out.push_str(&rect(-1, -1, VIEW_W + 2, 1, "#000000"));
    out.push_str(&rect(-1, VIEW_H, VIEW_W + 2, 1, "#000000"));
    out.push_str(&rect(-1, 0, 1, VIEW_H, "#000000"));
    out.push_str(&rect(VIEW_W, 0, 1, VIEW_H, "#000000"));
    for y in 0..VIEW_H {
        let mut x = 0;
        while x < VIEW_W {
            if shows(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < VIEW_W && !shows(x, y) {
                x += 1;
            }
            out.push_str(&rect(start, y, x - start, 1, "#000000"));
        }
    }
    out
}

/// A building standing on the sidewalk, with rows of windows, some of them lit.
fn building(x: i32, w: i32, h: i32, wall: &str, dark: &str, lit: &str, seed: i32) -> String {
    let top = GROUND_Y - h;
    let mut s = rect(x, top, w, h, wall);
    let mut wy = top + 3;
    while wy < GROUND_Y - 6 {
        let mut wx = x + 3;
        while wx < x + w - 3 {
            let on = noise(wx + seed, wy) > 0.8;
            s.push_str(&rect(wx, wy, 2, 2, if on { lit } else { dark }));
            wx += 5;
        }
        wy += 5;
    }
    s
}

/// The colours of one scene.
struct Palette {
    /// Bands of sky from the top down; empty leaves the page's black.
    sky: &'static [&'static str],
    star: &'static str,
    sun: &'static str,
    far_wall: &'static str,
    far_dark: &'static str,
    far_lit: &'static str,
    mid_wall: &'static str,
    mid_dark: &'static str,
    mid_lit: &'static str,
    awning: &'static str,
    shop: &'static str,
    pole: &'static str,
    lamp: &'static str,
    glow: bool,
    trunk: &'static str,
    leaves: &'static str,
    ground: &'static str,
    edge: &'static str,
    joint: &'static str,
    curb: &'static str,
    road: &'static str,
    lane: &'static str,
}

const NIGHT: Palette = Palette {
    sky: &[],
    star: "#4a4a4a",
    sun: "#d6d6d6",
    far_wall: "#0d0d0d",
    far_dark: "#151515",
    far_lit: "#262626",
    mid_wall: "#131313",
    mid_dark: "#1e1e1e",
    mid_lit: "#383838",
    awning: "#1d1d1d",
    shop: "#0a0a0a",
    pole: "#2e2e2e",
    lamp: "#d8d8d8",
    glow: true,
    trunk: "#2a2a2a",
    leaves: "#1b1f1b",
    ground: "#151515",
    edge: "#303030",
    joint: "#222222",
    curb: "#272727",
    road: "#0c0c0c",
    lane: "#3a3a3a",
};

const DAY: Palette = Palette {
    sky: &["#4f86b4", "#5f95c0", "#72a4cb", "#88b4d4", "#a0c4dc"],
    star: "#ffffff00",
    sun: "#ffe58a",
    far_wall: "#7f95aa",
    far_dark: "#6f86a0",
    far_lit: "#a8bfd4",
    mid_wall: "#8f8a86",
    mid_dark: "#6c7686",
    mid_lit: "#c9d6e2",
    awning: "#b5604f",
    shop: "#5a5450",
    pole: "#4a4a4e",
    lamp: "#4a4a4e",
    glow: false,
    trunk: "#6a4b34",
    leaves: "#4f8a4e",
    ground: "#b9b4aa",
    edge: "#d3cfc5",
    joint: "#9e998f",
    curb: "#8a857b",
    road: "#55565c",
    lane: "#d8d8d8",
};

fn build(c: &Palette, day: bool) -> String {
    // Far away: a tall skyline, with aerials on the highest towers.
    let mut skyline = String::new();
    for (i, (x, w, h)) in [(0, 16, 34), (18, 12, 24), (32, 20, 42), (54, 14, 28), (70, 18, 38), (90, 12, 22), (104, 14, 30)]
        .into_iter()
        .enumerate()
    {
        skyline.push_str(&building(x, w, h, c.far_wall, c.far_dark, c.far_lit, i as i32 * 13));
        if h > 36 {
            skyline.push_str(&rect(x + w / 2, GROUND_Y - h - 6, 1, 6, c.far_wall));
            skyline.push_str(&rect(x + w / 2, GROUND_Y - h - 7, 1, 1, if day { "#e8e8e8" } else { "#3a3a3a" }));
        }
    }
    // Nearer: shops and flats along the street, with awnings.
    let mut street = String::new();
    for (i, (x, w, h)) in [(0, 22, 28), (24, 14, 20), (40, 24, 32), (66, 16, 22), (84, 12, 26)]
        .into_iter()
        .enumerate()
    {
        street.push_str(&building(x, w, h, c.mid_wall, c.mid_dark, c.mid_lit, 100 + i as i32 * 7));
        street.push_str(&rect(x, GROUND_Y - 7, w, 2, c.awning));
        street.push_str(&rect(x + 2, GROUND_Y - 5, w - 4, 5, c.shop));
    }
    // On the sidewalk: a street lamp and a tree.
    let mut near = vec![
        rect(14, GROUND_Y - 30, 1, 30, c.pole),
        rect(14, GROUND_Y - 30, 8, 1, c.pole),
        rect(20, GROUND_Y - 29, 5, 2, c.lamp),
        rect(60, GROUND_Y - 12, 2, 12, c.trunk),
        rect(54, GROUND_Y - 24, 14, 6, c.leaves),
        rect(52, GROUND_Y - 20, 18, 6, c.leaves),
        rect(55, GROUND_Y - 15, 12, 4, c.leaves),
    ];
    if c.glow {
        near.push(rect(19, GROUND_Y - 27, 7, 1, "#ffffff22"));
        near.push(rect(18, GROUND_Y - 26, 9, 1, "#ffffff16"));
        near.push(rect(17, GROUND_Y - 25, 11, 1, "#ffffff0d"));
    }
    let near = near.concat();
    // The sky: bands in the day, a few stars at night; the moon or the sun.
    let mut sky = String::new();
    if !c.sky.is_empty() {
        let band = GROUND_Y / c.sky.len() as i32 + 1;
        for (i, colour) in c.sky.iter().enumerate() {
            sky.push_str(&rect(0, i as i32 * band, VIEW_W, band, colour));
        }
    }
    for (dy, from, to) in [(0, 2, 4), (1, 1, 5), (2, 0, 6), (3, 0, 6), (4, 0, 6), (5, 1, 5), (6, 2, 4)] {
        sky.push_str(&rect(132 + from, 1 + dy, to - from + 1, 1, c.sun));
    }
    if day {
        // Short rays round the sun.
        for (x, y) in [(135, 9), (127, 4), (143, 4)] {
            sky.push_str(&rect(x, y, 1, 2, c.sun));
        }
    } else {
        for (x, y) in [(20, 6), (46, 12), (78, 5), (104, 10), (128, 4), (146, 14), (186, 18), (12, 20), (60, 3)] {
            sky.push_str(&rect(x, y, 1, 1, c.star));
        }
    }
    // Clouds drifting by, in the day.
    let mut clouds = String::new();
    if day {
        let cloud = [
            rect(6, 12, 14, 3, "#f4f8fb"),
            rect(9, 10, 8, 2, "#f4f8fb"),
            rect(52, 20, 18, 3, "#f4f8fb"),
            rect(56, 18, 9, 2, "#f4f8fb"),
        ]
        .concat();
        clouds = format!("<g class=\"w-cloud\">{}</g>", repeat(&cloud, 100, 4));
    }
    // The sidewalk with a joint between its slabs every 16 pixels, the curb,
    // and the road with its lane marks every 24.
    let joint = rect(0, GROUND_Y + 1, 1, 5, c.joint);
    let lane = rect(0, GROUND_Y + 8, 10, 1, c.lane);
    let cat: String = (0..FRAMES)
        .map(|p| format!("<g class=\"wf wf{p}\">{}</g>", rects(&frame(p))))
        .collect();
    format!(
        "<svg class=\"walker-svg\" viewBox=\"-1 -1 {} {}\" xmlns=\"http://www.w3.org/2000/svg\" shape-rendering=\"crispEdges\">\
         {sky}{clouds}\
         <g class=\"w-far\">{}</g>\
         <g class=\"w-mid\">{}</g>\
         <g class=\"w-near\">{}</g>\
         {}{}{}{}\
         <g class=\"w-ground\">{}{}</g>\
         <g transform=\"translate(89 {})\">{cat}</g>\
         {}\
         </svg>",
        VIEW_W + 2,
        VIEW_H + 2,
        repeat(&skyline, 120, 3),
        repeat(&street, 96, 4),
        repeat(&near, 96, 4),
        rect(0, GROUND_Y, VIEW_W, VIEW_H - GROUND_Y, c.ground),
        rect(0, GROUND_Y, VIEW_W, 1, c.edge),
        rect(0, GROUND_Y + 6, VIEW_W, 1, c.curb),
        rect(0, GROUND_Y + 7, VIEW_W, VIEW_H - GROUND_Y - 7, c.road),
        repeat(&joint, 16, 16),
        repeat(&lane, 24, 10),
        GROUND_Y - H as i32,
        edge_cover(),
    )
}

fn scene(day: bool) -> &'static str {
    static NIGHT_SCENE: OnceLock<String> = OnceLock::new();
    static DAY_SCENE: OnceLock<String> = OnceLock::new();
    if day {
        DAY_SCENE.get_or_init(|| super::pixels::compact(&build(&DAY, true)))
    } else {
        NIGHT_SCENE.get_or_init(|| super::pixels::compact(&build(&NIGHT, false)))
    }
}

/// The cat walking along the street, at night or (with `day`) in daylight.
#[component]
pub fn WalkingCat(#[props(default = false)] day: bool) -> Element {
    rsx! { div { class: "walker", dangerous_inner_html: scene(day) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_is_drawn_and_they_differ() {
        let frames: Vec<String> = (0..FRAMES).map(|p| rects(&frame(p))).collect();
        assert!(frames.iter().all(|f| f.contains("<rect")));
        for a in 0..FRAMES {
            for b in a + 1..FRAMES {
                assert_ne!(frames[a], frames[b], "poses {a} and {b} look the same");
            }
        }
        assert!(scene(false).contains("w-ground") && scene(true).contains("w-cloud"));
    }

    #[test]
    fn the_edges_break_up_and_the_middle_stays_whole() {
        // Whole around the cat, bare at the far sides and bottom.
        assert!((60..140).all(|x| (20..50).all(|y| shows(x, y))));
        assert!((0..2).all(|x| (0..VIEW_H).all(|y| !shows(x, y))));
        assert!((0..VIEW_W).all(|x| !shows(x, VIEW_H - 1)));
        assert!((0..VIEW_W).all(|x| !shows(x, 0)), "the top edge is bare too");
        assert!((60..140).all(|x| shows(x, 14)), "and thickens quickly");
        // Somewhere in between, some pixels show and some do not.
        let shown = (2..40).flat_map(|x| (10..40).map(move |y| (x, y))).filter(|&(x, y)| shows(x, y)).count();
        assert!(shown > 0 && shown < 38 * 30);
    }

    #[test]
    fn the_street_loops_without_a_seam() {
        // What moves does so by a whole number of the pattern's repeats per
        // cycle: the sidewalk slabs (16) and lane marks (24) by 48, the lamps
        // and shops (96) by 96, the skyline (120) by 120.
        for (period, travel) in [(16, 48), (24, 48), (96, 96), (120, 120)] {
            assert_eq!(travel % period, 0);
        }
        // 8, 4 and 2 pixels a pose: the street never moves by part of a pixel.
        assert_eq!((48 / FRAMES, 96 / 12, 120 / 60), (8, 8, 2));
    }
}
