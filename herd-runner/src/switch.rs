//! The owner's switch, on the owner's machine (E-33).
//!
//! E-33 asks for two ways to move one switch: the owner, from the machine
//! itself, and an orchestrator, through the hub. This module is the first, and
//! it is the state both of them leave behind.
//!
//! **It is a file, not a port.** Invariant 3 says a runner dials out and never
//! listens, so there is nothing on this machine for `herd-runner switch off`
//! to connect *to*: it writes one small file under the runner's data root, and
//! the running runner reads that file on a short clock. Three things fall out
//! of that choice, and all three are wanted:
//!
//! * it works with the hub down, which is exactly when an owner is most likely
//!   to want her machine back;
//! * it survives the runner process — an owner who switched her PC off before
//!   rebooting it has not changed her mind, and a runner that rejoined `on`
//!   would be a machine that gave itself back;
//! * a switch set through the hub is written here too, so there is one state
//!   and not two.
//!
//! The file is read **permissively in one direction only**: anything that is
//! not a position this Herd understands leaves the switch where it is and says
//! so loudly, because the dangerous reading is the other one. A file that
//! would not parse read as `on` would be a withdrawn machine quietly claiming
//! work again.

use std::path::{Path, PathBuf};
use std::time::Duration;

use herd_proto::Switch;
use serde::{Deserialize, Serialize};

/// How often the running runner looks at its switch file.
///
/// One second. E-33's figure is ten, and this is the whole of what a runner
/// spends out of it — the rest is the frame reaching the hub and the hub
/// re-judging its queue, which happen at once. A `stat` and a short read of a
/// file in the page cache once a second costs nothing measurable, and the
/// alternative — `inotify` — is a dependency and a platform surface for a
/// saving nobody can observe.
pub const DEFAULT_POLL_MS: u64 = 1_000;

/// The file's name under the runner's data root.
pub const SWITCH_FILE: &str = "switch";

/// Where the switch file is, given a data root.
pub fn default_path(data_root: &Path) -> PathBuf {
    data_root.join(SWITCH_FILE)
}

/// The switch file's contents: the position, and why.
///
/// JSON, with the position's own words, so an owner can read and write it with
/// `cat` and `echo` when she wants to — the same reasoning D8 gives for a job
/// record being plain files.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnDisk {
    position: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// What a switch file says, or why it does not say anything usable.
///
/// `Ok(None)` is a file that is not there, which is a machine nobody has
/// withdrawn. `Err` is a file that exists and cannot be read as a position —
/// and the caller's rule for that is never "assume `on`".
pub fn read(path: &Path) -> Result<Option<Switch>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{} cannot be read: {error}", path.display())),
    };
    if text.trim().is_empty() {
        return Err(format!(
            "{} is empty, which is not a position: a machine nobody has withdrawn has no \
             switch file at all",
            path.display()
        ));
    }
    let read: OnDisk = serde_json::from_str(&text)
        .map_err(|error| format!("{} will not read: {error}", path.display()))?;
    Switch::parse(&read.position, read.reason.as_deref())
        .map(Some)
        .map_err(|why| format!("{}: {why}", path.display()))
}

/// Write a position down, so that it outlives the process that applied it.
pub fn write(path: &Path, switch: &Switch) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{} cannot be made: {error}", parent.display()))?;
    }
    let on_disk = OnDisk {
        position: switch.name().to_owned(),
        reason: switch.reason().map(str::to_owned),
    };
    let text = serde_json::to_string_pretty(&on_disk)
        .map_err(|error| format!("a switch will not encode: {error}"))?;
    // Written beside and renamed over, because a reader is polling this file
    // once a second and a half-written one is one that will not read. The
    // reading is safe either way — an unreadable switch file leaves the
    // position where it is — but "safe" here would mean an owner's last word
    // taking a second longer than she was promised.
    let beside = path.with_extension("new");
    std::fs::write(&beside, text.as_bytes())
        .map_err(|error| format!("{} cannot be written: {error}", beside.display()))?;
    std::fs::rename(&beside, path).map_err(|error| {
        format!(
            "{} cannot be renamed over {}: {error}",
            beside.display(),
            path.display()
        )
    })
}

/// How long the runner waits between two readings of its switch file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Poll(pub Duration);

impl Default for Poll {
    fn default() -> Self {
        Self(Duration::from_millis(DEFAULT_POLL_MS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_a_temporary_place(contents: Option<&str>) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = default_path(directory.path());
        if let Some(contents) = contents {
            std::fs::write(&path, contents).expect("the fixture writes the switch file");
        }
        (directory, path)
    }

    /// No file is a machine nobody has withdrawn.
    #[test]
    fn an_absent_switch_file_is_a_runner_that_claims() {
        let (_keep, path) = in_a_temporary_place(None);
        assert_eq!(read(&path), Ok(None));
    }

    /// A position and its reason survive a round trip, which is what makes a
    /// switch outlive the process.
    #[test]
    fn a_written_switch_reads_back_with_its_reason() {
        let (_keep, path) = in_a_temporary_place(None);
        let off = Switch::Off {
            reason: "she is gaming".to_owned(),
        };
        write(&path, &off).expect("the switch is written");
        assert_eq!(read(&path), Ok(Some(off)));
    }

    /// **The reading that must never happen.** Every unusable file is an
    /// error, and no unusable file is `on`: a withdrawn machine whose switch
    /// file was truncated, mangled or written with a position this Herd does
    /// not know must not quietly start claiming work again.
    #[test]
    fn nothing_unreadable_reads_as_a_runner_that_claims() {
        for contents in [
            "",
            "   \n",
            "not json at all",
            r#"{"position":"of course"}"#,
            r#"{"position":"off"}"#,
            r#"{"position":"on","reason":"a reason on means nothing by"}"#,
            r#"{}"#,
        ] {
            let (_keep, path) = in_a_temporary_place(Some(contents));
            let read = read(&path);
            assert!(
                read.is_err(),
                "{contents:?} should not be readable as a position, and it read as {read:?}"
            );
            assert_ne!(
                read.ok().flatten(),
                Some(Switch::On),
                "{contents:?} must never read as a runner that claims"
            );
        }
    }

    /// `off` needs a reason and `on` refuses one, in the one place both the
    /// file and the two command lines go through.
    #[test]
    fn a_position_carries_exactly_the_reason_it_should() {
        assert_eq!(Switch::parse("on", None), Ok(Switch::On));
        assert!(Switch::parse("on", Some("why")).is_err());
        assert!(Switch::parse("off", None).is_err());
        assert!(Switch::parse("drain", None).is_err());
        assert!(Switch::parse("off", Some("  ")).is_err());
        assert_eq!(
            Switch::parse("off", Some(" she is gaming ")),
            Ok(Switch::Off {
                reason: "she is gaming".to_owned()
            })
        );
    }

    /// What each position does, asserted on the gates themselves rather than
    /// on a comment: `off` stops claiming **and** stops serving content
    /// (D11), a drain stops claiming and leaves when it is empty, and `on`
    /// does neither.
    #[test]
    fn each_position_gates_exactly_what_it_should() {
        let off = Switch::Off {
            reason: "hers".to_owned(),
        };
        assert!(!off.claims());
        assert!(!off.serves_content(), "off must stop serving content (D11)");
        assert!(!off.leaves_when_empty());

        let draining = Switch::Drain {
            reason: "hers".to_owned(),
        };
        assert!(!draining.claims());
        assert!(draining.serves_content());
        assert!(draining.leaves_when_empty());

        assert!(Switch::On.claims());
        assert!(Switch::On.serves_content());
        assert!(!Switch::On.leaves_when_empty());
    }
}
