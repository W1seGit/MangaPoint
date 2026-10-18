use super::*;
use std::collections::HashMap;
use pipeline::{ChapterRef, ChapterRequest, SourceChoice};

fn request(n: u32) -> ChapterRequest {
    ChapterRequest {
        number: Some(n as f64),
        name: format!("Chapter {n}"),
        key: Some(format!("c{n}")),
        options: vec![ChapterRef {
            source_id: "s".into(),
            chapter_id: n as i64,
        }],
    }
}

fn batch(series: &str, chapters: &[u32]) -> Batch {
    Batch::new(
        series.into(),
        series.to_uppercase(),
        chapters.iter().map(|&n| request(n)).collect(),
        chapters.iter().map(|n| format!("c{n}")).collect(),
        SourceChoice::Adaptive,
        HashMap::new(),
        0,
    )
}

#[test]
fn batches_run_one_after_another_without_anyone_starting_them() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1, 2]));
    q.add(batch("b", &[1]));
    let first = q.next_to_start().unwrap();
    q.start(first);
    assert_eq!(q.next_to_start(), None, "one at a time");
    q.finish(first, BatchStatus::Done, "ok".into(), vec![], vec![]);
    let second = q.next_to_start().unwrap();
    assert_ne!(first, second);
    assert_eq!(q.get(second).unwrap().series_id, "b");
}

#[test]
fn chapters_added_while_a_series_waits_join_its_batch_without_repeats() {
    let mut q = DownloadQueue::default();
    let first = q.add(batch("a", &[1, 2]));
    assert_eq!(
        first,
        Added {
            chapters: 2,
            joined_existing: false
        }
    );
    let more = q.add(batch("a", &[2, 3, 3, 4]));
    assert_eq!(
        more,
        Added {
            chapters: 2,
            joined_existing: true
        },
        "2 is already queued"
    );
    assert_eq!(q.batches.len(), 1);
    assert_eq!(q.batches[0].plan, ["c1", "c2", "c3", "c4"]);
    assert_eq!(
        q.batches[0].requests.len(),
        4,
        "requests stay aligned with the plan"
    );
    assert_eq!(q.add(batch("a", &[1, 4])).chapters, 0);
}

#[test]
fn chapters_added_while_the_series_is_running_wait_in_a_new_batch() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1, 2]));
    let id = q.next_to_start().unwrap();
    q.start(id);
    let added = q.add(batch("a", &[2, 3]));
    assert_eq!(
        added,
        Added {
            chapters: 1,
            joined_existing: false
        },
        "2 is being downloaded"
    );
    assert_eq!(q.batches.len(), 2);
    assert_eq!(q.batches[1].plan, ["c3"]);
}

#[test]
fn pausing_stops_new_batches_from_starting_but_not_the_running_one() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1]));
    q.add(batch("b", &[1]));
    let id = q.next_to_start().unwrap();
    q.start(id);
    q.paused = true;
    assert!(q.running().is_some());
    q.finish(id, BatchStatus::Done, String::new(), vec![], vec![]);
    assert_eq!(q.next_to_start(), None);
    q.paused = false;
    assert!(q.next_to_start().is_some());
}

#[test]
fn cancelling_drops_a_waiting_batch_and_stops_a_running_one() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1]));
    q.add(batch("b", &[1]));
    let a = q.batches[0].id;
    let b = q.batches[1].id;
    q.start(a);
    assert!(q.cancel(b));
    assert!(q.get(b).is_none(), "the waiting one is gone");
    assert!(q.cancel(a));
    assert!(q.get(a).unwrap().cancel.is_cancelled());
    assert!(q.get(a).unwrap().progress.stopping);
    assert!(!q.cancel(999));
}

#[test]
fn cancel_all_clears_the_line_and_stops_what_is_running() {
    let mut q = DownloadQueue::default();
    for s in ["a", "b", "c"] {
        q.add(batch(s, &[1]));
    }
    let a = q.batches[0].id;
    q.start(a);
    q.cancel_all();
    assert_eq!(q.batches.len(), 1);
    assert!(q.batches[0].cancel.is_cancelled());
}

#[test]
fn stopping_one_series_leaves_the_others_alone() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1]));
    q.add(batch("b", &[1]));
    q.add(batch("a", &[5]));
    let first = q.batches[0].id;
    q.start(first); // "a", running; the joined chapter 5 is in the same batch
    q.add(batch("a", &[9])); // a second, waiting batch for "a"
    q.cancel_series("a");
    let left: Vec<&str> = q.batches.iter().map(|b| b.series_id.as_str()).collect();
    assert_eq!(
        left,
        ["a", "b"],
        "the running one stays (it is stopping), the waiting one went"
    );
    assert!(q.batches[0].cancel.is_cancelled());
    assert!(!q.batches[1].cancel.is_cancelled());
    assert!(q.series_has_work("b"));
}

#[test]
fn a_batch_can_be_moved_to_the_front_of_the_line() {
    let mut q = DownloadQueue::default();
    for s in ["a", "b", "c"] {
        q.add(batch(s, &[1]));
    }
    let (a, c) = (q.batches[0].id, q.batches[2].id);
    q.start(a);
    q.prioritize(c);
    let order: Vec<&str> = q.batches.iter().map(|b| b.series_id.as_str()).collect();
    assert_eq!(
        order,
        ["a", "c", "b"],
        "after the running one, before the rest"
    );
    q.prioritize(a);
    assert_eq!(q.batches[0].series_id, "a", "a running batch is not moved");
}

#[test]
fn finished_batches_can_be_cleared_and_retried() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1, 2, 3]));
    let id = q.next_to_start().unwrap();
    q.start(id);
    q.finish(
        id,
        BatchStatus::Done,
        "2 of 3".into(),
        vec!["Chapter 2: no source".into()],
        vec!["c2".into()],
    );
    assert_eq!(q.get(id).unwrap().chapters_done(), 2);
    assert!(!q.remove(999));

    let queued = q.retry(id, 5);
    assert_eq!(queued, 1);
    assert_eq!(q.batches.len(), 2);
    assert_eq!(q.batches[1].plan, ["c2"]);
    assert_eq!(q.batches[1].status, BatchStatus::Waiting);
    assert!(q.get(id).unwrap().failed.is_empty());
    assert_eq!(q.retry(id, 6), 0, "nothing left to retry");

    q.clear_finished();
    assert_eq!(
        q.batches.len(),
        1,
        "the finished one is gone, the waiting one stays"
    );
    assert!(q.has_work());
}

#[test]
fn a_chapters_state_follows_the_queue_and_active_work_beats_an_old_failure() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1, 2]));
    assert_eq!(q.chapter_state("a", "c1"), Some(QueuedChapter::Waiting));
    assert_eq!(q.chapter_state("a", "c9"), None);
    assert_eq!(q.chapter_state("other", "c1"), None);

    let id = q.next_to_start().unwrap();
    q.start(id);
    q.get_mut(id)
        .unwrap()
        .progress
        .apply(&pipeline::JobEvent::ChapterStarted {
            index: 0,
            total: 2,
            name: "Chapter 1".into(),
        });
    q.get_mut(id)
        .unwrap()
        .progress
        .apply(&pipeline::JobEvent::PageDone {
            chapter_index: 0,
            done: 4,
            total: 10,
        });
    assert_eq!(
        q.chapter_state("a", "c1"),
        Some(QueuedChapter::Downloading {
            pages_done: 4,
            pages_total: 10
        })
    );
    assert_eq!(q.chapter_state("a", "c2"), Some(QueuedChapter::Waiting));

    q.finish(
        id,
        BatchStatus::Done,
        String::new(),
        vec![],
        vec!["c2".into()],
    );
    assert_eq!(q.chapter_state("a", "c2"), Some(QueuedChapter::Failed));
    // Trying again puts it back in the queue: that wins over the old failure.
    q.retry(id, 1);
    assert_eq!(q.chapter_state("a", "c2"), Some(QueuedChapter::Waiting));
}

#[test]
fn counts_say_how_much_is_left() {
    let mut q = DownloadQueue::default();
    assert!(!q.has_work());
    q.add(batch("a", &[1, 2, 3]));
    q.add(batch("b", &[1, 2]));
    assert_eq!(q.counts(), (0, 2, 5));
    let id = q.next_to_start().unwrap();
    q.start(id);
    assert_eq!(q.counts(), (1, 1, 5));
    assert!(q.series_has_work("a") && q.series_has_work("b"));
    q.finish(id, BatchStatus::Cancelled, String::new(), vec![], vec![]);
    assert!(!q.series_has_work("a"));
    assert_eq!(q.counts(), (0, 1, 2));
}

#[test]
fn a_saved_queue_comes_back_with_its_finished_batches_and_resumes_the_rest() {
    let mut q = DownloadQueue::default();
    for title in ["Done one", "Waits", "Was running"] {
        q.add(Batch::new(
            title.to_lowercase(),
            title.into(),
            vec![ChapterRequest {
                number: Some(1.0),
                name: "c".into(),
                key: Some(format!("{title}-1")),
                options: Vec::new(),
            }],
            vec![format!("{title}-1")],
            SourceChoice::Only("src".into()),
            HashMap::new(),
            10,
        ));
    }
    let (a, c) = (q.batches[0].id, q.batches[2].id);
    q.finish(
        a,
        BatchStatus::Done,
        "Downloaded 1 chapter".into(),
        Vec::new(),
        Vec::new(),
    );
    q.set_stats(a, BatchStats { secs: 9, pages: 40 });
    q.start(c);

    let json = serde_json::to_string(&q.to_saved()).unwrap();
    let back = DownloadQueue::from_saved(serde_json::from_str(&json).unwrap());
    assert_eq!(back.batches.len(), 3);
    assert_eq!(back.batches[0].status, BatchStatus::Done);
    assert_eq!(
        back.batches[0].stats,
        Some(BatchStats { secs: 9, pages: 40 })
    );
    assert!(back.batches[0].finished_at > 0);
    assert_eq!(
        back.batches[2].status,
        BatchStatus::Waiting,
        "it starts again"
    );
    assert_eq!(back.batches[1].choice, SourceChoice::Only("src".into()));
    assert!(!back.runner_active);

    // Lists that do not match are cut to the shorter, not trusted.
    let mut odd = q.to_saved();
    odd.batches[0].plan.push("extra".into());
    let odd = DownloadQueue::from_saved(odd);
    assert_eq!(odd.batches[0].plan.len(), odd.batches[0].requests.len());
}

#[test]
fn a_batch_can_be_put_aside_and_goes_on_with_what_is_left() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1, 2, 3]));
    q.add(batch("b", &[1]));
    let (a, b) = (q.batches[0].id, q.batches[1].id);
    // A waiting batch is paused at once and is skipped by the line.
    assert!(q.pause(b));
    assert_eq!(q.next_to_start(), Some(a));
    // A running one is told to stop, and is parked with what is not saved.
    q.start(a);
    assert!(q.pause(a));
    assert!(q.get(a).unwrap().pausing);
    let saved: std::collections::HashSet<String> = [q.get(a).unwrap().plan[0].clone()].into();
    q.park(a, &saved);
    let parked = q.get(a).unwrap();
    assert_eq!(parked.status, BatchStatus::Paused);
    assert_eq!(parked.plan.len(), 2);
    assert!(!parked.pausing && !parked.cancel.is_cancelled());
    assert!(parked.status.is_active(), "it still counts as work to do");
    assert_eq!(q.next_to_start(), None, "nothing runs until one is resumed");
    assert!(q.resume(a));
    assert_eq!(q.next_to_start(), Some(a));
    // Giving a paused one up drops it like a waiting one.
    assert!(q.cancel(b));
    assert!(q.get(b).is_none());
}

#[test]
fn a_cancel_after_a_pause_wins_and_a_pause_after_a_cancel_does_not_park() {
    let mut q = DownloadQueue::default();
    q.add(batch("a", &[1, 2]));
    q.add(batch("b", &[1, 2]));
    let (a, b) = (q.batches[0].id, q.batches[1].id);
    q.start(a);
    assert!(q.pause(a));
    q.cancel_all();
    assert!(!q.get(a).unwrap().pausing, "the cancel clears the pending pause");
    assert!(q.get(a).unwrap().cancel.is_cancelled());
    assert!(q.get(b).is_none());
    q.finish(a, BatchStatus::Cancelled, "x".into(), vec![], vec![]);
    assert!(!q.series_has_work("a"));

    q.add(batch("c", &[1]));
    let c = q.batches.last().unwrap().id;
    q.start(c);
    assert!(q.cancel(c));
    assert!(!q.pause(c), "a batch that is stopping for good cannot be parked");
    assert!(!q.get(c).unwrap().pausing);
    q.finish(c, BatchStatus::Cancelled, "x".into(), vec![], vec![]);

    q.add(batch("d", &[1]));
    let d = q.batches.last().unwrap().id;
    q.start(d);
    q.pause(d);
    q.cancel_series("d");
    assert!(!q.get(d).unwrap().pausing);
}
