//! What the progress bar and its words show while a job runs.

use pipeline::JobEvent;

/// One chapter being worked on right now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActiveChapter {
    pub name: String,
    pub pages_done: usize,
    pub pages_total: usize,
}

/// What the progress bar shows. Chapters can download together, so it keeps
/// every chapter in progress, not just one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobProgress {
    pub chapter_total: usize,
    /// Chapters completely dealt with.
    pub finished: usize,
    /// Chapters under way, by their position in the job.
    pub active: std::collections::BTreeMap<usize, ActiveChapter>,
    pub note: Option<String>,
    /// When the first chapter began, and how many pages have arrived since.
    pub started: Option<std::time::Instant>,
    pub pages_seen: usize,
    /// The user pressed cancel; the job is winding down.
    pub stopping: bool,
    /// Set once the finished files are being sent to their target.
    pub writing: Option<WriteProgress>,
    /// After a pause: the chapters that were done and the total, so the numbers do not start over
    /// while the converted chapters are read back in.
    pub resumed: (usize, usize),
}

/// How far the finished files have got to the drive, folder or reader.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriteProgress {
    /// The file being sent now.
    pub file: String,
    /// Bytes stored so far, of all the files, and the bytes there are in all.
    pub done: u64,
    pub total: u64,
    /// The drive has everything and is being made to keep it.
    pub flushing: bool,
    /// When the first file began.
    began: Option<std::time::Instant>,
    /// Recent (time, bytes stored), for a speed that follows the drive as it
    /// slows down or speeds up instead of averaging the whole copy.
    samples: std::collections::VecDeque<(std::time::Instant, u64)>,
}

impl WriteProgress {
    /// How long back the speed looks.
    const WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

    fn record(&mut self, done: u64, total: u64) {
        let now = std::time::Instant::now();
        self.done = done;
        self.total = total;
        self.samples.push_back((now, done));
        while self.samples.len() > 2
            && self
                .samples
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at) > Self::WINDOW)
        {
            self.samples.pop_front();
        }
    }

    /// Bytes a second over the whole copy, for the log.
    pub fn average_bytes_per_second(&self) -> Option<f64> {
        let seconds = self.began?.elapsed().as_secs_f64();
        (seconds > 0.0 && self.done > 0).then(|| self.done as f64 / seconds)
    }

    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f32 / self.total as f32).clamp(0.0, 1.0))
    }

    /// Bytes a second over the last few seconds, once there is enough to say.
    pub fn bytes_per_second(&self) -> Option<f64> {
        let (first_at, first) = *self.samples.front()?;
        let (last_at, last) = *self.samples.back()?;
        let seconds = last_at.duration_since(first_at).as_secs_f64();
        (seconds >= 0.5 && last > first).then(|| (last - first) as f64 / seconds)
    }

    /// Seconds left at the current speed.
    pub fn eta_secs(&self) -> Option<u64> {
        let speed = self.bytes_per_second()?;
        (self.done < self.total).then(|| ((self.total - self.done) as f64 / speed).ceil() as u64)
    }
}

impl JobProgress {
    pub fn apply(&mut self, event: &JobEvent) {
        match event {
            JobEvent::ChapterStarted { index, total, name } => {
                self.chapter_total = *total;
                self.started.get_or_insert_with(std::time::Instant::now);
                self.active.insert(
                    *index,
                    ActiveChapter {
                        name: name.clone(),
                        ..Default::default()
                    },
                );
                self.note = None;
            }
            JobEvent::PageDone {
                chapter_index,
                done,
                total,
            } => {
                self.pages_seen += 1;
                let chapter = self.active.entry(*chapter_index).or_default();
                chapter.pages_done = *done;
                chapter.pages_total = *total;
            }
            JobEvent::UsedFallback { to, .. } => {
                self.note = Some(format!("That source failed, trying {to}."));
            }
            JobEvent::ChapterSkipped { name, .. } => {
                self.note = Some(format!("Skipped {name}: no source could supply it."));
            }
            JobEvent::ChapterSaved { .. } => {}
            JobEvent::ChapterDone { index } => {
                if self.active.remove(index).is_some() {
                    self.finished += 1;
                }
            }
            JobEvent::PagesAtOnce { now, chosen } => {
                self.note = (now < chosen).then(|| {
                    format!(
                        "The source is pushing back, so downloads are slowed to {now} at a time."
                    )
                });
            }
            JobEvent::Flushing => {
                if let Some(writing) = self.writing.as_mut() {
                    writing.flushing = true;
                }
            }
            JobEvent::Writing { file } => {
                self.active.clear();
                self.finished = self.chapter_total;
                self.note = None;
                let writing = self.writing.get_or_insert_with(WriteProgress::default);
                writing.file = file.clone();
                writing.flushing = false;
                writing.began.get_or_insert_with(std::time::Instant::now);
            }
            JobEvent::Sent { done, total } => {
                if let Some(writing) = self.writing.as_mut() {
                    writing.record(*done, *total);
                }
            }
        }
    }

    /// How fast pages are arriving, once there is enough to say.
    pub fn pages_per_second(&self) -> Option<f32> {
        if self.writing.is_some() {
            return None;
        }
        let seconds = self.started?.elapsed().as_secs_f32();
        (seconds >= 4.0 && self.pages_seen >= 6).then(|| self.pages_seen as f32 / seconds)
    }

    /// Seconds left at the pace so far, once there is enough to say.
    pub fn eta_secs(&self) -> Option<u64> {
        if self.writing.is_some() {
            return None;
        }
        let seconds = self.started?.elapsed().as_secs_f32();
        let done = self.fraction();
        (seconds >= 4.0 && (0.03..1.0).contains(&done))
            .then(|| (seconds * (1.0 - done) / done).round() as u64)
    }

    /// Chapters done, counting those done before a pause.
    pub fn done(&self) -> usize {
        self.finished.max(self.resumed.0)
    }

    /// Chapters in the job, as far as known.
    pub fn total(&self) -> usize {
        self.chapter_total.max(self.resumed.1)
    }

    /// Overall progress, 0.0 to 1.0.
    pub fn fraction(&self) -> f32 {
        // Once the files are being sent, the bar is about that.
        if let Some(fraction) = self.writing.as_ref().and_then(WriteProgress::fraction) {
            return fraction;
        }
        if self.total() == 0 {
            return 0.0;
        }
        let before = if self.resumed.1 > 0 { self.resumed.0 as f32 / self.resumed.1 as f32 } else { 0.0 };
        let partial: f32 = self
            .active
            .values()
            .filter(|c| c.pages_total > 0)
            .map(|c| c.pages_done as f32 / c.pages_total as f32)
            .sum();
        (((self.finished as f32 + partial) / self.chapter_total.max(1) as f32).max(before)).clamp(0.0, 1.0)
    }

    pub fn label(&self) -> String {
        if self.stopping {
            return "Stopping…".into();
        }
        if self.chapter_total == 0 {
            return "Starting…".into();
        }
        match self.active.len() {
            0 if self.finished == 0 => "Starting…".into(),
            0 => format!("{} of {} chapters done", self.finished, self.chapter_total),
            1 => {
                let (index, chapter) = self.active.iter().next().expect("one entry");
                let pages = if chapter.pages_total > 0 {
                    format!(", page {} of {}", chapter.pages_done, chapter.pages_total)
                } else {
                    String::new()
                };
                format!("Chapter {} of {}{pages}", index + 1, self.chapter_total)
            }
            n => format!(
                "{} of {} chapters done, {n} in progress",
                self.finished, self.chapter_total
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(index: usize, total: usize) -> JobEvent {
        JobEvent::ChapterStarted {
            index,
            total,
            name: format!("Ch {}", index + 1),
        }
    }

    fn pages(chapter_index: usize, done: usize, total: usize) -> JobEvent {
        JobEvent::PageDone {
            chapter_index,
            done,
            total,
        }
    }

    #[test]
    fn progress_combines_chapter_and_page_position() {
        let mut p = JobProgress::default();
        assert_eq!(p.fraction(), 0.0);
        assert_eq!(p.label(), "Starting…");
        p.apply(&started(1, 4));
        assert_eq!(p.fraction(), 0.0);
        p.apply(&pages(1, 5, 10));
        assert!((p.fraction() - 0.125).abs() < 1e-6);
        assert_eq!(p.label(), "Chapter 2 of 4, page 5 of 10");
        p.apply(&JobEvent::UsedFallback {
            chapter_index: 1,
            from: "a".into(),
            to: "b".into(),
            reason: String::new(),
        });
        assert!(p.note.as_deref().unwrap().contains('b'));
        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.fraction(), 0.25);
        p.apply(&started(2, 4));
        assert_eq!(p.note, None, "the note clears with the next chapter");
        assert_eq!(p.label(), "Chapter 3 of 4");
    }

    #[test]
    fn chapters_downloading_together_share_the_bar() {
        let mut p = JobProgress::default();
        for i in 0..3 {
            p.apply(&started(i, 6));
        }
        p.apply(&pages(0, 5, 10));
        p.apply(&pages(1, 10, 10));
        p.apply(&pages(2, 0, 10));
        assert_eq!(p.active.keys().copied().collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(p.label(), "0 of 6 chapters done, 3 in progress");
        assert!((p.fraction() - 1.5 / 6.0).abs() < 1e-6);

        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.active.keys().copied().collect::<Vec<_>>(), [0, 2]);
        assert_eq!(p.label(), "1 of 6 chapters done, 2 in progress");
        assert!(
            (p.fraction() - 1.5 / 6.0).abs() < 1e-6,
            "one chapter's 100% moved from partial to finished"
        );
        // A second "done" for the same chapter is not counted twice.
        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.finished, 1);
    }

    #[test]
    fn easing_off_is_explained_and_clears_when_the_source_recovers() {
        let mut p = JobProgress::default();
        p.apply(&started(0, 1));
        p.apply(&JobEvent::PagesAtOnce { now: 3, chosen: 12 });
        assert!(p.note.as_deref().unwrap().contains('3'));
        p.apply(&JobEvent::PagesAtOnce {
            now: 12,
            chosen: 12,
        });
        assert_eq!(p.note, None);
    }

    #[test]
    fn progress_never_leaves_zero_to_one() {
        let p = JobProgress {
            chapter_total: 2,
            finished: 9,
            ..Default::default()
        };
        assert_eq!(p.fraction(), 1.0);
    }

    #[test]
    fn a_stopping_job_says_so_whatever_it_was_doing() {
        let mut p = JobProgress::default();
        p.apply(&JobEvent::ChapterStarted {
            index: 0,
            total: 3,
            name: "Ch 1".into(),
        });
        assert_eq!(p.label(), "Chapter 1 of 3");
        p.stopping = true;
        assert_eq!(p.label(), "Stopping…");
    }

    #[test]
    fn no_time_left_is_guessed_before_there_is_a_pace() {
        let mut p = JobProgress::default();
        assert_eq!(p.eta_secs(), None);
        p.chapter_total = 10;
        p.finished = 5;
        assert_eq!(p.eta_secs(), None, "nothing has started yet");
    }

    #[test]
    fn the_bar_follows_the_bytes_stored_once_files_are_being_sent() {
        let mut p = JobProgress {
            chapter_total: 4,
            finished: 4,
            ..Default::default()
        };
        assert_eq!(p.fraction(), 1.0, "all chapters converted");
        p.apply(&JobEvent::Writing { file: "a.xtc".into() });
        p.apply(&JobEvent::Sent { done: 0, total: 1000 });
        assert_eq!(p.fraction(), 0.0, "nothing written yet");
        p.apply(&JobEvent::Sent { done: 250, total: 1000 });
        assert_eq!(p.fraction(), 0.25);
        assert!(p.pages_per_second().is_none() && p.eta_secs().is_none());
        p.apply(&JobEvent::Flushing);
        assert!(p.writing.as_ref().unwrap().flushing);
    }

    #[test]
    fn speed_needs_a_little_time_and_progress() {
        let mut w = WriteProgress::default();
        assert!(w.bytes_per_second().is_none());
        w.record(0, 100);
        assert!(w.bytes_per_second().is_none(), "one sample says nothing");
        assert_eq!(w.eta_secs(), None);
    }
}
