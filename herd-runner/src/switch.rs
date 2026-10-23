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
//!   would be a machine that gave itself back. Two of the three positions
//!   survive as themselves; a persisted `drain` is read as `off`, because it
//!   was an instruction to a process that is gone ([`at_startup`], D164);
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

/// What a position written in the file means to a runner that is only now
/// starting (D164).
///
/// Two of the three positions mean the same thing at startup as they mean to a
/// running runner, and `drain` does not. It is an instruction to *one process*
/// — finish what you hold, then go — and a process that has just started holds
/// nothing, so it has already carried it out. Read literally it is a runner
/// that joins and leaves again, which under a unit with `Restart = "always"`
/// is a restart loop, and a runner nobody can reach to switch back `on`
/// because it is never connected long enough for the instruction to land.
///
/// `off` is what a restart can mean: nothing new is claimed, the reason the
/// owner gave stays visible, the runner stays connected, and that last clause
/// is the point — `herdctl switch <runner> on` reaches it from anywhere,
/// instead of only from a shell on the machine.
pub struct AtStartup {
    /// The position this process takes.
    pub position: Switch,
    /// Why that is not the position written down, when it is not. One
    /// sentence, said once on the runner's own stderr: an owner who wrote
    /// `drain` and gets `off` has to be able to read why.
    pub converted: Option<String>,
}

/// Read a persisted position as a starting runner reads it (D164).
pub fn at_startup(written: Switch) -> AtStartup {
    let Switch::Drain { reason } = written else {
        return AtStartup {
            position: written,
            converted: None,
        };
    };
    let off = Switch::Off {
        reason: reason.clone(),
    };
    AtStartup {
        converted: Some(format!(
            "the switch file says drain ({reason}), and a drain is an instruction to one \
             process: finish what you hold, then go. This process has just started and holds \
             nothing, so it has already done both — so it starts {}, which is the position a \
             restart can keep: nothing new is claimed, the reason stays visible, this runner \
             stays connected, and `herdctl switch <runner> on` can reach it from anywhere. A \
             runner that read drain here would leave the moment it joined, and a unit that \
             restarts it would do that for ever",
            off.sentence()
        )),
        position: off,
    }
}

/// Write a position down, so that it outlives the process that applied it.
///
/// **Durably**, since D99: `herdctl switch` now reports success on the strength
/// of this returning `Ok`, so `Ok` has to mean the bytes are on the disk and
/// not in a page cache that a lost machine takes with it. Written beside,
/// fsynced, renamed over, and the directory fsynced — the discipline
/// `herd_hub::durable` uses for a job's record, for the same reason.
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
    std::fs::File::open(&beside)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("{} cannot be fsynced: {error}", beside.display()))?;
    std::fs::rename(&beside, path).map_err(|error| {
        format!(
            "{} cannot be renamed over {}: {error}",
            beside.display(),
            path.display()
        )
    })?;
    // The rename itself has to be on the disk too: without this the file is
    // there for anything that reads it and gone after a power cut, and D99's
    // promise is about a machine that comes back.
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("{} cannot be fsynced: {error}", parent.display()))?;
    }
    Ok(())
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

        // And the one case that moved out of that list under D166. This file
        // is not mangled: it says `on` in so many words and carries one
        // surplus field, so reading it as `on` is reading what it says rather
        // than guessing at it. The hazard above is untouched — a truncated
        // file, an unknown position and a missing one are all still errors —
        // and the hazard here was never the same thing: nothing in Herd
        // writes a reason on `on`, so such a file was typed by a person, and
        // refusing it would withdraw a machine nobody meant to withdraw.
        let (_keep, path) =
            in_a_temporary_place(Some(r#"{"position":"on","reason":"the window is over"}"#));
        assert_eq!(read(&path), Ok(Some(Switch::On)));
    }

    /// **The restart loop, closed.** A `drain` on the disk is read as `off`
    /// by the process that finds it there (D164).
    ///
    /// The outage: `herdctl switch server drain`, then `systemctl restart`.
    /// Each start read `drain`, held nothing, and left at once; the unit's
    /// `Restart = "always"` started it again, eight times in two minutes, and
    /// `herdctl switch server on` could not land because the runner was never
    /// connected long enough to be told. Read as `off` the same machine stays
    /// connected with its owner's reason visible, and the instruction lands.
    #[test]
    fn a_persisted_drain_starts_a_runner_off_and_not_leaving() {
        let (_keep, path) = in_a_temporary_place(None);
        let drain = Switch::Drain {
            reason: "the rig is mine tonight".to_owned(),
        };
        write(&path, &drain).expect("the switch is written");
        let written = read(&path)
            .expect("the file reads")
            .expect("the file holds a position");
        assert_eq!(written, drain, "the file keeps what was written to it");

        let starting = at_startup(written);
        assert_eq!(
            starting.position,
            Switch::Off {
                reason: "the rig is mine tonight".to_owned()
            },
            "a persisted drain starts off, with the reason its owner gave"
        );
        assert!(
            !starting.position.leaves_when_empty(),
            "a runner that starts on this position must not leave the moment it joins"
        );
        assert!(!starting.position.claims(), "and it claims nothing either");
        let said = starting
            .converted
            .expect("an owner who wrote drain and gets off must be told why");
        for word in ["drain", "off", "connected", "restart"] {
            assert!(
                said.contains(word),
                "the sentence has to say {word:?} to explain itself: {said}"
            );
        }
        assert!(
            said.contains("the rig is mine tonight"),
            "and it carries her reason: {said}"
        );
    }

    /// The other two positions mean at startup what they mean at any other
    /// time, and say nothing: `off` is the position that survives a restart,
    /// and `on` is a machine nobody has withdrawn.
    #[test]
    fn the_other_two_positions_survive_a_restart_as_themselves() {
        for position in [
            Switch::On,
            Switch::Off {
                reason: "she is gaming".to_owned(),
            },
        ] {
            let starting = at_startup(position.clone());
            assert_eq!(starting.position, position);
            assert!(
                starting.converted.is_none(),
                "nothing was converted, so nothing is explained: {:?}",
                starting.converted
            );
        }
    }

    /// `off` needs a reason and `on` keeps none, in the one place both the
    /// file and the two command lines go through.
    #[test]
    fn a_position_carries_exactly_the_reason_it_should() {
        assert_eq!(Switch::parse("on", None), Ok(Switch::On));
        // A reason on `on` is accepted and dropped, never a refusal (D166):
        // the position carries none, and failing a command because it was
        // given one extra piece of information was a bad trade.
        assert_eq!(
            Switch::parse("on", Some("the window is over")),
            Ok(Switch::On)
        );
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
