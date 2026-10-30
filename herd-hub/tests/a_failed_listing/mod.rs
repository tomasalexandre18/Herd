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
        if std::fs::read_dir(&jobs).is_ok() {
            std::fs::set_permissions(&jobs, std::fs::Permissions::from_mode(0o755))
                .expect("the jobs directory is given back its permissions");
            return None;
        }
        Some(Self { jobs })
    }

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
