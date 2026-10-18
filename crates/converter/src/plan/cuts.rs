//! Cutting a strip into pages by looking at the whole chapter at once.
//!
//! Cutting one page at a time takes the latest quiet place it can find near
//! the bottom of the screen, and whatever that leaves for the next page is
//! the next page's problem. Here every cut is chosen together: each page
//! should come close to filling the screen, and each cut should fall in a blank
//! gap or a quiet row. The best set of cuts is found by dynamic programming.

use super::*;

/// The cost of a page a whole screen short, squared: pages are scored by how
/// far from full they are.
const SHORT_PAGE_COST: f32 = 1.0;
/// The last page is short as a matter of course, so its shortfall counts for less.
const LAST_PAGE_SHARE: f32 = 0.3;
/// Cutting anywhere but a blank gap costs at least this much, so a gap is
/// taken whenever the page it ends is more than about half full.
const NOT_A_GAP_COST: f32 = 0.2;
/// On top of that, the cost of cutting through a row as busy as [`BUSY_ROW`] or more.
const BUSY_CUT_COST: f32 = 1.2;
/// A row with this much difference between neighbouring pixels (on average) is as bad to cut as any.
const BUSY_ROW: f32 = 12.0;
/// The shortest page that is not the last, as a share of the screen height.
const MIN_FILL_PERCENT: u32 = 40;
/// The cost of cutting through lettering, or this close to it. Far above
/// any other cost, so it happens only when every other place is lettering too.
const LETTERING_CUT_COST: f32 = 3.0;
/// A bubble has about this much padding round its letters, in rows.
const LETTERING_MARGIN: u32 = 16;

/// A place a page can end, and where the next one then starts.
struct Cut {
    /// The first row the page before leaves out.
    ends: u32,
    /// The first row of the page after. Later than `ends` when a blank gap is skipped.
    starts: u32,
    cost: f32,
    /// This cut is a blank gap, so a page may end here however short it is.
    gap: bool,
}

/// Cut `strip` into pages of at most one screen, ending each in a blank gap
/// or a quiet row. Nothing is repeated from one page to the next.
pub(super) fn plan_quiet(strip: &Strip, opts: &ProcessOptions) -> ChapterPlan {
    let screen = opts.height;
    let total = strip.total;
    if total == 0 {
        return ChapterPlan {
            strip: true,
            pages: Vec::new(),
            unit_height: 0,
        };
    }
    let min_gutter = (screen / 60).max(4);
    let pad = (screen / 100).clamp(2, 10);

    let width = strip.width.max(1) as f32;
    let activity: Vec<f32> = (0..total)
        .map(|y| {
            if strip.blank[y as usize] {
                0.0
            } else {
                row_activity(strip, y) as f32 / width
            }
        })
        .collect();
    // A row and its neighbours, so one quiet row between two lines of
    // lettering does not win.
    let busyness = |y: u32| {
        let (lo, hi) = (y.saturating_sub(1) as usize, (y + 1).min(total - 1) as usize);
        activity[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32
    };

    // Per row: the cost of cutting through lettering, 0 where there is none.
    let mut lettering = vec![0.0f32; total as usize];
    for (y0, y1) in lettering::lettered_spans(strip, screen) {
        let (y0, y1) = (y0.saturating_sub(LETTERING_MARGIN), (y1 + LETTERING_MARGIN).min(total));
        lettering[y0 as usize..y1 as usize].fill(LETTERING_CUT_COST);
    }

    // Every row is a place to cut, except in a blank gap, where one cut stands
    // for the whole gap and the middle of it is left out.
    let mut cuts = vec![Cut { ends: 0, starts: 0, cost: 0.0, gap: true }];
    let mut y = 1;
    while y < total {
        if strip.blank[y as usize] {
            let first = y;
            while y < total && strip.blank[y as usize] {
                y += 1;
            }
            if y - first >= min_gutter && y < total {
                let ends = (first + pad).min(y);
                cuts.push(Cut { ends, starts: y.saturating_sub(pad).max(ends), cost: 0.0, gap: true });
            } else {
                cuts.extend((first..y).map(|row| Cut { ends: row, starts: row, cost: 0.0, gap: false }));
            }
            continue;
        }
        let through = (BUSY_CUT_COST * (busyness(y) / BUSY_ROW).min(1.0)).max(lettering[y as usize]);
        let cost = NOT_A_GAP_COST + through;
        cuts.push(Cut { ends: y, starts: y, cost, gap: false });
        y += 1;
    }
    let last = cuts.len();
    cuts.push(Cut { ends: total, starts: total, cost: 0.0, gap: true });

    let full = screen as f32;
    let shortest = (screen * MIN_FILL_PERCENT / 100) as i64;
    let mut best = vec![f32::INFINITY; cuts.len()];
    let mut from = vec![usize::MAX; cuts.len()];
    best[0] = 0.0;
    for i in 1..cuts.len() {
        let ends = cuts[i].ends as i64;
        let is_last = i == last;
        for j in (0..i).rev() {
            let height = ends - cuts[j].starts as i64;
            if height > screen as i64 {
                break;
            }
            if height <= 0 || best[j].is_infinite() || (height < shortest && !cuts[i].gap) {
                continue;
            }
            let short = (full - height as f32) / full;
            let share = if is_last { LAST_PAGE_SHARE } else { 1.0 };
            let cost = best[j] + SHORT_PAGE_COST * share * short * short + cuts[i].cost;
            if cost < best[i] {
                best[i] = cost;
                from[i] = j;
            }
        }
    }

    // Every row is a place to cut, and a page may end in a gap however short it
    // is, so the end can always be reached. If it somehow is not, the older cut still gives pages.
    if best[last].is_infinite() {
        return plan_overlap(strip, opts);
    }
    let mut chain = vec![last];
    while let Some(&i) = chain.last() {
        if i == 0 {
            break;
        }
        chain.push(from[i]);
    }
    chain.reverse();
    let pages = chain
        .windows(2)
        .map(|pair| (cuts[pair[0]].starts, cuts[pair[1]].ends - cuts[pair[0]].starts))
        // Pages that are only background are dropped: trailing whitespace.
        .filter(|&(top, height)| (top..top + height).any(|y| !strip.blank[y as usize]))
        .map(|(top, height)| PlannedPage {
            source: Source::Strip { top, height },
            unit_top: to_units(top, strip.width),
            unit_height: to_units(height, strip.width),
            landscape: false,
        })
        .collect();
    ChapterPlan {
        strip: true,
        pages,
        unit_height: to_units(total, strip.width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dither::Dither;
    use crate::gray::Depth;

    fn opts() -> ProcessOptions {
        let mut o = ProcessOptions::manga_default(480, 800, Depth::Two);
        o.layout = Layout::Webtoon;
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.dither = Dither::Threshold;
        o.strip_cut = StripCut::Quiet;
        o
    }

    /// Stripes with a white gap of `gap` rows after every `scene` rows.
    fn strip_with_gaps(rows: u32, scene: u32, gap: u32) -> GrayImage {
        GrayImage::from_fn(480, rows, |x, y| {
            if y % scene >= scene - gap {
                255
            } else if (x / 3) % 2 == 0 {
                40
            } else {
                220
            }
        })
        .unwrap()
    }

    fn spans(plan: &ChapterPlan) -> Vec<(u32, u32)> {
        plan.pages
            .iter()
            .map(|p| match p.source {
                Source::Strip { top, height } => (top, height),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn no_page_is_taller_than_the_screen_and_nothing_is_skipped() {
        let images = [strip_with_gaps(9000, 530, 40)];
        let o = opts();
        let plan = plan(&prepare(&images, &o), &o);
        let spans = spans(&plan);
        assert!(spans.iter().all(|&(_, h)| h <= 800), "{spans:?}");
        // Pages follow one another, and only a blank gap is ever left out.
        for pair in spans.windows(2) {
            assert!(pair[1].0 >= pair[0].0 + pair[0].1, "overlap: {pair:?}");
            assert!(pair[1].0 - (pair[0].0 + pair[0].1) <= 40, "content skipped: {pair:?}");
        }
        let (top, height) = *spans.last().unwrap();
        assert!(top + height >= 9000 - 40);
    }

    #[test]
    fn pages_end_in_the_gaps_between_scenes() {
        // A 60-row gap ends every 530-row scene.
        let images = [strip_with_gaps(8000, 530, 60)];
        let o = opts();
        let plan = plan(&prepare(&images, &o), &o);
        for (top, height) in spans(&plan).iter().take(8) {
            let end = top + height;
            let in_gap = (0..=10).any(|d| end >= d && (end - d) % 530 >= 530 - 60);
            assert!(in_gap, "page {top}+{height} ends at {end}, not in a gap");
        }
    }

    #[test]
    fn pages_are_nearly_full() {
        // A gap every 330 rows: two scenes make 660, three would not fit.
        // Cutting after each scene would leave every page 40% full.
        let images = [strip_with_gaps(9900, 330, 30)];
        let o = opts();
        let plan = plan(&prepare(&images, &o), &o);
        let spans = spans(&plan);
        let (all_but_last, _) = spans.split_at(spans.len() - 1);
        assert!(all_but_last.iter().all(|&(_, h)| h >= 600), "{spans:?}");
    }

    #[test]
    fn a_little_content_between_big_gaps_still_gets_its_own_pages() {
        // 100 rows of drawing, then 500 blank, again and again: a page can
        // be no fuller than a few hundred rows, but nothing may be lost.
        let images = [GrayImage::from_fn(480, 6000, |x, y| {
            if y % 600 >= 100 {
                255
            } else if (x / 3) % 2 == 0 {
                40
            } else {
                220
            }
        })
        .unwrap()];
        let o = opts();
        let plan = plan(&prepare(&images, &o), &o);
        let spans = spans(&plan);
        for scene in 0..10u32 {
            let rows = scene * 600..scene * 600 + 100;
            assert!(
                rows.clone().all(|y| spans.iter().any(|&(t, h)| (t..t + h).contains(&y))),
                "scene {scene} is not on a page: {spans:?}"
            );
        }
    }

    #[test]
    fn a_strip_shorter_than_a_screen_is_one_page() {
        let images = [strip_with_gaps(500, 100_000, 0)];
        let o = opts();
        let plan = plan(&prepare(&images, &o), &o);
        assert_eq!(spans(&plan), vec![(0, 500)]);
    }

    #[test]
    fn a_strip_of_nothing_but_background_has_no_pages() {
        let images = [GrayImage::new(480, 3000, 255).unwrap()];
        let o = opts();
        assert!(plan(&prepare(&images, &o), &o).pages.is_empty());
    }
}

#[cfg(test)]
mod lettering_tests {
    use super::*;
    use crate::dither::Dither;
    use crate::gray::Depth;

    /// Busy stripes everywhere, and a line of lettering across rows 700..760,
    /// right where a full page would end.
    fn strip_with_lettering() -> GrayImage {
        GrayImage::from_fn(480, 3000, |x, y| {
            let letter = (700..760).contains(&y) && (x % 24) < 14 && !((x % 24) >= 4 && (x % 24) < 10 && (y - 700) % 30 >= 6 && (y - 700) % 30 < 24);
            if letter {
                0
            } else if (700..760).contains(&y) {
                255
            } else if (x / 3) % 2 == 0 {
                60
            } else {
                200
            }
        })
        .unwrap()
    }

    #[test]
    fn a_cut_keeps_away_from_lettering() {
        let mut o = ProcessOptions::manga_default(480, 800, Depth::Two);
        o.layout = Layout::Webtoon;
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.dither = Dither::Threshold;
        o.strip_cut = StripCut::Quiet;
        let plan = plan(&prepare(&[strip_with_lettering()], &o), &o);
        let Source::Strip { height, .. } = plan.pages[0].source else {
            unreachable!()
        };
        assert!(!(700..=760).contains(&height), "the first page ends at {height}, in the lettering");
        assert!(height > 760, "the page should run past the lettering, not stop short of it: {height}");
    }
}
