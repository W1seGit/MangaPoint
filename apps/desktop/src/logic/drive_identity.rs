//! Telling whether the drive an interrupted export was sending to is still the
//! same one. Drive letters are handed out again: a different card or stick
//! plugged in later can get the letter the reader had.

use pipeline::export::Drive;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What is kept of a drive to recognise it again: its label and its size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveIdentity {
    pub name: String,
    pub total_bytes: u64,
}

impl DriveIdentity {
    pub fn describe(&self) -> String {
        let name = if self.name.trim().is_empty() {
            "no name"
        } else {
            self.name.trim()
        };
        format!("\"{name}\", {}", super::human_bytes(self.total_bytes))
    }
}

/// The plugged-in drive that holds `place` (the deepest mount point it is under).
pub fn identity_of(place: &Path, drives: &[Drive]) -> Option<DriveIdentity> {
    drives
        .iter()
        .filter(|d| place.starts_with(&d.mount))
        .max_by_key(|d| d.mount.components().count())
        .map(|d| DriveIdentity {
            name: d.name.clone(),
            total_bytes: d.total_bytes,
        })
}

#[derive(Debug, PartialEq, Eq)]
pub enum DriveCheck {
    /// Nothing was kept to compare with (a folder, or an older saved export).
    Unchecked,
    Same,
    /// No plugged-in drive holds the place now.
    Missing(DriveIdentity),
    /// The place is on a different drive than when the export began.
    Changed {
        was: DriveIdentity,
        now: DriveIdentity,
    },
}

pub fn check_drive(saved: Option<&DriveIdentity>, place: &Path, drives: &[Drive]) -> DriveCheck {
    let Some(was) = saved else {
        return DriveCheck::Unchecked;
    };
    match identity_of(place, drives) {
        None => DriveCheck::Missing(was.clone()),
        Some(now) if now == *was => DriveCheck::Same,
        Some(now) => DriveCheck::Changed {
            was: was.clone(),
            now,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn drive(mount: &str, name: &str, gb: u64) -> Drive {
        Drive {
            mount: PathBuf::from(mount),
            name: name.into(),
            total_bytes: gb * 1_000_000_000,
            available_bytes: 0,
        }
    }

    // Paths with forward slashes, which every platform splits into folders.
    #[test]
    fn a_letter_that_now_belongs_to_another_drive_is_noticed() {
        let reader = DriveIdentity {
            name: "XTEINK".into(),
            total_bytes: 8_000_000_000,
        };
        let place = Path::new("/media/reader/books");
        let same = [drive("/media/reader", "XTEINK", 8)];
        assert_eq!(identity_of(place, &same).as_ref(), Some(&reader));
        assert_eq!(check_drive(Some(&reader), place, &same), DriveCheck::Same);
        let other = [drive("/media/reader", "USB DISK", 64)];
        assert!(matches!(
            check_drive(Some(&reader), place, &other),
            DriveCheck::Changed { .. }
        ));
        // Same label, other size: a different card.
        let bigger = [drive("/media/reader", "XTEINK", 32)];
        assert!(matches!(
            check_drive(Some(&reader), place, &bigger),
            DriveCheck::Changed { .. }
        ));
        assert!(matches!(
            check_drive(Some(&reader), place, &[drive("/media/other", "XTEINK", 8)]),
            DriveCheck::Missing(_)
        ));
        assert_eq!(check_drive(None, place, &other), DriveCheck::Unchecked);
    }

    #[test]
    fn the_drive_that_holds_a_place_is_the_deepest_mount() {
        let drives = [drive("/", "root", 500), drive("/media/reader", "XTEINK", 8)];
        let got = identity_of(Path::new("/media/reader/books"), &drives).unwrap();
        assert_eq!(got.name, "XTEINK");
    }

    #[test]
    fn a_drive_is_described_for_the_question_asked() {
        let id = DriveIdentity {
            name: " ".into(),
            total_bytes: 8_000_000_000,
        };
        assert!(id.describe().starts_with("\"no name\""));
    }
}
