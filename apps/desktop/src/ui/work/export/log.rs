//! The log lines for a job's events.

use crate::ui::work::*;

/// One readable line per meaningful step; page progress every 25 pages of
/// each chapter (chapters can download together, so progress is kept per chapter).
pub(in crate::ui::work) fn log_event(event: &JobEvent, logged_pages: &mut HashMap<usize, usize>) {
    match event {
        JobEvent::ChapterStarted { index, total, name } => {
            logged_pages.insert(*index, 0);
            info!("Chapter {}/{}: {name}", index + 1, total);
        }
        JobEvent::PageDone {
            chapter_index,
            done,
            total,
        } => {
            let last = logged_pages.entry(*chapter_index).or_insert(0);
            if done / 25 > *last / 25 || done == total {
                info!("  chapter {}: {done}/{total} pages", chapter_index + 1);
            }
            *last = *done;
        }
        JobEvent::UsedFallback {
            from, to, reason, ..
        } => {
            warn!("  source {from} failed ({reason}); trying {to}");
        }
        JobEvent::ChapterSkipped { name, reason, .. } => {
            warn!("  skipped {name}: {reason}");
        }
        JobEvent::ChapterSaved { chapter } => {
            info!(
                "  saved {} ({} pages, {})",
                chapter.name,
                chapter.pages,
                logic::human_bytes(chapter.bytes)
            );
        }
        JobEvent::ChapterDone { .. } => {}
        JobEvent::PagesAtOnce { now, chosen } => {
            if now < chosen {
                warn!("  the source is pushing back: {now} requests at once instead of {chosen}");
            } else {
                info!("  the source recovered: back to {chosen} requests at once");
            }
        }
        JobEvent::Writing { file } => info!("Writing {file}"),
        JobEvent::Sent { .. } => {}
        JobEvent::Flushing => info!("The file is copied; waiting for the drive to store it"),
    }
}
