#![allow(dead_code)]
//! **The fixture for a hub whose own `readdir` fails**, shared by the two
//! tests that need it (ORCH-197, D225).
//!
//! The jobs directory is left with execute permission and no read
//! permission, which is what a `chmod`, an ACL or a tree another user owns
//! does to a hub. `fs::read_dir` then fails with `EACCES` while
//! `open("jobs/<id>/job.json")` still succeeds — and that gap is the whole of
//! the fix: the hub cannot enumerate, so it claims nothing about any
//! collection, and each one a runner *offers* is answered from that job's own
//! record, which the failed listing says nothing about.
//!
//! **A real permission and never a mock.** A mocked `read_dir` would prove
//! something about the mock. What has to hold is that one syscall fails while
//! another on the same tree succeeds, and only the kernel can say that.
//!
//! **And a real permission is one something else can take back.** This
//! suite runs inside a Herd job, and the runner carrying that job hands
//! itself read access back on any directory under the job it cannot list,
//! every 200 ms, because invariant 2 and D166 say a job must not `chmod` its
//! way out of a figure the runner enforces. So this fixture is not a state a
//! test may set once and then reason from: it is a state every measurement
//! taken through it has to be **checked against**, which is what
//! [`AListingThatFails::still_binds`] is for and where the whole mechanism
//! is written down.
//!
//! It lives in its own module because two tests need the same absence and a
//! second copy of it is a second thing to keep true (D79's shape, applied to
//! a fixture): `owed_scan_fails.rs` drives the hub as a library object
//! against a hand-written peer, and
//! `owed_scan_fails_against_the_real_runner.rs` drives both sides as
//! processes. The permission, the guard that gives it back and the
//! *"but a record still opens"* half are one definition for both.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use herd_proto::JobId;

/// The jobs directory with its read permission taken away, put back when the
/// test ends however it ends.
///
/// A guard rather than two calls, because a panicking assertion would
/// otherwise leave a directory the temporary-directory cleanup cannot walk.
pub struct AListingThatFails {
    jobs: PathBuf,
}

impl AListingThatFails {
    /// `None` when this process is one permissions do not apply to, which is
    /// a skip and not a pass (D27): there is no way to make a real `readdir`
    /// fail for root, and a mock of one would prove nothing about the hub.
    pub fn over(state_dir: &Path) -> Option<Self> {
        let jobs = state_dir.join("jobs");
        std::fs::set_permissions(&jobs, std::fs::Permissions::from_mode(0o111))
            .expect("the jobs directory's permissions are the test's to set");
        match std::fs::read_dir(&jobs) {
            // It lists at 0o111: a process permissions do not apply to.
            Ok(_) => {
                std::fs::set_permissions(&jobs, std::fs::Permissions::from_mode(0o755))
                    .expect("the jobs directory is given back its permissions");
                None
            }
            // It fails *because of the mode*, which is the fixture. Any
            // other error is a listing that failed for a reason nobody here
            // arranged — a descriptor limit, a tree that moved — and
            // reading it as the fixture would put a green or a red on
            // something nobody built.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                Some(Self { jobs })
            }
            Err(error) => panic!(
                "{} would not list for a reason this fixture did not arrange: {error:?}",
                jobs.display()
            ),
        }
    }

    /// Whether the listing is **still** failing — asked at the moment a
    /// measurement is judged, never assumed from `over` having succeeded.
    ///
    /// **A mode this suite sets is not a mode this suite keeps.** Every
    /// assertion here runs inside a Herd job (D258), and the runner carrying
    /// that job walks everything under the job's base every
    /// `herd_runner::confine::POLL_INTERVAL` — 200 ms — to hold it to its
    /// disk budget: `herd_runner::root::measure_tree` over
    /// `Workspace::base_dir`, and `tmp` is inside that base. A directory
    /// that walk cannot list has the runner's own access handed back to it
    /// by `herd_runner::root::read_dir_restoring`, which calls
    /// `restore_access`: `set_permissions(mode | 0o700)`. Every temporary
    /// state root in this suite lives under `$TMPDIR`, so `0o111` becomes
    /// `0o711` within 200 ms and the listing works again.
    ///
    /// That is the runner doing exactly what invariant 2 and D166 require —
    /// a job must not `chmod` its way out of a figure the runner enforces —
    /// so the fixture is the side that has to be honest about it. A hub that
    /// scans after the mode came back scans *successfully* and says what it
    /// is owed, which is correct for the permissions it really had and is no
    /// evidence at all about ORCH-197.
    ///
    /// Measured on 2026-10-30 in job j0000000147, a directory moded 0o111
    /// under that job's `$TMPDIR`: `mode=2111 read_dir=EACCES` at t=0.10 s,
    /// `mode=2711 read_dir=ok` at t=0.20 s. `0o711` where the fixture set
    /// `0o111` — never the guard's own `0o755` — is the fingerprint.
    pub fn still_binds(&self) -> bool {
        matches!(
            std::fs::read_dir(&self.jobs),
            Err(ref error) if error.kind() == std::io::ErrorKind::PermissionDenied
        )
    }

    /// Break the listing again, for a measurement [`Self::still_binds`]
    /// found void. Not a retry of an assertion: a retry of the **fixture**,
    /// after which the measurement is taken again from nothing.
    pub fn hold_again(&self) {
        std::fs::set_permissions(&self.jobs, std::fs::Permissions::from_mode(0o111))
            .expect("the jobs directory's permissions are the test's to set");
    }

    /// How many measurements are taken before this host is called one that
    /// will not hold the fixture at all.
    ///
    /// Eight, against a window of a few milliseconds and a walk every
    /// 200 ms: one collision is a few percent and eight in a row is not a
    /// number this project waits for. The bound exists so that a host which
    /// restores access faster than the fixture can set it is **red with a
    /// sentence** rather than looping — red and not skipped, because a skip
    /// would move the suite's skip list, and that list is compared and not
    /// counted (D27, D187).
    pub const ATTEMPTS: usize = 8;

    /// What a test says when every one of those attempts was undone.
    pub const WOULD_NOT_HOLD: &'static str =
        "this host handed the jobs directory its read permission back inside every \
         attempt, so the hub never scanned a broken listing and nothing here is evidence \
         about ORCH-197. See AListingThatFails::still_binds: the runner carrying this \
         suite restores access on any directory under the job it cannot list, every \
         200 ms, and 0o711 where the fixture set 0o111 is the fingerprint";

    /// The sentence a test prints instead of passing, when this host is one
    /// the fixture cannot be built on (D27).
    pub const WHY_IT_WOULD_BE_SKIPPED: &'static str =
        "this process is one file permissions do not apply to, so no real readdir can be \
         made to fail here";

    /// The other half of the fixture, asserted rather than assumed: a record
    /// still opens **by name**. If this ever stopped holding, the fixture
    /// would be testing a hub with no records at all, and the fix it is about
    /// would have nothing to read.
    pub fn but_a_record_still_opens(&self, job_id: &JobId) {
        let record = self.jobs.join(job_id.as_str()).join("job.json");
        assert!(
            std::fs::read_to_string(&record).is_ok(),
            "the fixture is a listing that fails, not a record that is gone: {} must still \
             open by name",
            record.display()
        );
    }
}

impl Drop for AListingThatFails {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.jobs, std::fs::Permissions::from_mode(0o755));
    }
}
