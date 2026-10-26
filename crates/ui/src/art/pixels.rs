//! Pixel pictures drawn as SVG are written one `<rect>` per run of pixels, and a
//! scene can come to thousands of them. The browser has to read, style and lay
//! out every one before the page can slide in, which showed as a hitch. This
//! joins the rectangles of the same colour into a single path each, leaving the
//! picture exactly as it was: a rectangle is only moved up to an earlier one of
//! its colour when nothing drawn after that one touches it.

/// One colour's rectangles, as the path that draws them.
struct Item {
    fill: String,
    path: String,
    /// Every rectangle in `path`, as left, top, right, bottom.
    boxes: Vec<(i32, i32, i32, i32)>,
}

impl Item {
    fn touches(&self, (l, t, r, b): (i32, i32, i32, i32)) -> bool {
        self.boxes
            .iter()
            .any(|&(sl, st, sr, sb)| l < sr && sl < r && t < sb && st < b)
    }
}

/// Rectangles waiting to be written, in the order they are drawn.
#[derive(Default)]
struct Run(Vec<Item>);

impl Run {
    fn add(&mut self, x: i32, y: i32, w: i32, h: i32, fill: &str) {
        let bounds = (x, y, x + w, y + h);
        let path = format!("M{x} {y}h{w}v{h}h-{w}z");
        for item in self.0.iter_mut().rev() {
            if item.fill == fill {
                item.path.push_str(&path);
                item.boxes.push(bounds);
                return;
            }
            // Something of another colour is drawn over this place later:
            // joining an earlier rectangle would put this one underneath it.
            if item.touches(bounds) {
                break;
            }
        }
        self.0.push(Item {
            fill: fill.to_string(),
            path,
            boxes: vec![bounds],
        });
    }

    fn write(&mut self, out: &mut String) {
        for item in self.0.drain(..) {
            out.push_str(&format!("<path fill=\"{}\" d=\"{}\"/>", item.fill, item.path));
        }
    }
}

/// The text of the attribute `name` (as `name="…"`) in a tag.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let from = tag.find(&key)? + key.len();
    let len = tag[from..].find('"')?;
    Some(&tag[from..from + len])
}

/// The same picture with each run of rectangles written as a few paths.
/// Markup that is not made of `<rect x y width height fill/>` is left alone.
pub fn compact(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len() / 3);
    let mut run = Run::default();
    let mut rest = svg;
    while let Some(start) = rest.find('<') {
        if start > 0 {
            run.write(&mut out);
            out.push_str(&rest[..start]);
        }
        let tag_start = &rest[start..];
        let Some(end) = tag_start.find('>') else {
            break;
        };
        let tag = &tag_start[..=end];
        let number = |name| attribute(tag, name).and_then(|v| v.parse::<i32>().ok());
        let rect = if tag.starts_with("<rect ") {
            match (number("x"), number("y"), number("width"), number("height"), attribute(tag, "fill")) {
                (Some(x), Some(y), Some(w), Some(h), Some(fill)) => Some((x, y, w, h, fill)),
                _ => None,
            }
        } else {
            None
        };
        match rect {
            Some((x, y, w, h, fill)) => run.add(x, y, w, h, fill),
            None => {
                run.write(&mut out);
                out.push_str(tag);
            }
        }
        rest = &tag_start[end + 1..];
    }
    run.write(&mut out);
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// What the picture shows: the colour on each pixel after everything is
    /// painted in order, from rectangles or from the paths that replace them.
    fn painted(svg: &str) -> HashMap<(i32, i32), String> {
        let mut shown = HashMap::new();
        let mut rest = svg;
        while let Some(at) = rest.find('<') {
            let tag = &rest[at..at + rest[at..].find('>').unwrap() + 1];
            rest = &rest[at + tag.len()..];
            let fill = attribute(tag, "fill").unwrap_or("").to_string();
            let mut boxes: Vec<(i32, i32, i32, i32)> = Vec::new();
            if tag.starts_with("<rect ") {
                let n = |name| attribute(tag, name).unwrap().parse::<i32>().unwrap();
                boxes.push((n("x"), n("y"), n("width"), n("height")));
            } else if tag.starts_with("<path ") {
                for part in attribute(tag, "d").unwrap().split('M').skip(1) {
                    let (point, sizes) = part.split_once('h').unwrap();
                    let (x, y) = point.split_once(' ').unwrap();
                    let (w, rest) = sizes.split_once('v').unwrap();
                    let (h, _) = rest.split_once('h').unwrap();
                    boxes.push((x.parse().unwrap(), y.parse().unwrap(), w.parse().unwrap(), h.parse().unwrap()));
                }
            }
            for (x, y, w, h) in boxes {
                for yy in y..y + h {
                    for xx in x..x + w {
                        shown.insert((xx, yy), fill.clone());
                    }
                }
            }
        }
        shown
    }

    #[test]
    fn the_picture_is_the_same_with_far_fewer_elements() {
        // A deterministic pile of overlapping rectangles of a few colours.
        let colours = ["#111", "#222", "#333", "#444"];
        let mut svg = String::from("<svg viewBox=\"0 0 40 40\">");
        let mut seed = 12_345u32;
        let mut next = |m: u32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 16) % m
        };
        for _ in 0..400 {
            let (x, y, w, h) = (next(36), next(36), 1 + next(5), 1 + next(5));
            let fill = colours[next(4) as usize];
            svg.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>"));
        }
        svg.push_str("<g class=\"x\"><rect x=\"1\" y=\"1\" width=\"2\" height=\"2\" fill=\"#999\"/></g></svg>");
        let small = compact(&svg);
        assert_eq!(painted(&svg), painted(&small));
        assert!(small.len() < svg.len(), "{} -> {}", svg.len(), small.len());
        assert!(small.matches('<').count() < svg.matches('<').count() / 3);
        // Groups and the rest of the markup are untouched.
        assert!(small.starts_with("<svg viewBox=\"0 0 40 40\">") && small.contains("<g class=\"x\">"));
    }

    #[test]
    fn a_rectangle_never_slips_under_one_drawn_over_it() {
        // Red, then blue on top of it, then red again over the blue: the last
        // red must stay on top, so it cannot join the first.
        let svg = "<rect x=\"0\" y=\"0\" width=\"4\" height=\"4\" fill=\"red\"/>\
                   <rect x=\"1\" y=\"1\" width=\"2\" height=\"2\" fill=\"blue\"/>\
                   <rect x=\"2\" y=\"2\" width=\"1\" height=\"1\" fill=\"red\"/>";
        let small = compact(svg);
        assert_eq!(painted(svg), painted(&small));
        assert_eq!(small.matches("<path").count(), 3);
        // Away from the blue, it can join.
        let apart = "<rect x=\"0\" y=\"0\" width=\"1\" height=\"1\" fill=\"red\"/>\
                     <rect x=\"5\" y=\"5\" width=\"1\" height=\"1\" fill=\"blue\"/>\
                     <rect x=\"9\" y=\"9\" width=\"1\" height=\"1\" fill=\"red\"/>";
        assert_eq!(compact(apart).matches("<path").count(), 2);
        assert_eq!(painted(apart), painted(&compact(apart)));
    }
}
