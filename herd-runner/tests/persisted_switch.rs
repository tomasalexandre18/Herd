//! A position on the disk, read by a runner that is only now starting (D164).
//!
//! No hub here, and that is the point: the restart loop this closes happened
//! with the hub up and had nothing to do with it. A runner that reads `drain`
//! from its own file leaves before it has done anything at all, and the thing
//! to assert is that it does not — which a dialling runner with nowhere to
//! dial demonstrates perfectly well.

use std::time::Duration;

use herd_proto::Switch;
use herd_runner::testing::TempDir;

/// The outage of 2026-10-23, as a test.
///
/// `herdctl switch server drain` and then `systemctl restart`: every start
/// read the persisted `drain`, held nothing, and left at once, which under
/// `Restart = "always"` is eight starts in two minutes and a runner nobody
/// can reach to switch back on. So: a runner started on a persisted `drain`
/// must still be running, and its file must say `off`.
#[tokio::test(flavor = "multi_thread")]
async fn a_runner_started_on_a_persisted_drain_stays_and_does_not_leave() {
    let home = TempDir::new("persisted-drain");
    let switch_file = herd_runner::switch::default_path(home.path());
    herd_runner::switch::write(
        &switch_file,
        &Switch::Drain {
            reason: "the rig is mine tonight".to_owned(),
        },
    )
    .expect("the fixture writes the position its owner set");

    let data_root = TempDir::new("persisted-drain-root");
    let runner = tokio::spawn(herd_runner::run_until(
        herd_runner::Config {
            // Nothing is listening there, deliberately: this runner will
            // redial for ever, and a runner that leaves while it is doing
            // that left for the reason under test and no other.
            hub_url: "ws://127.0.0.1:1/herd".to_owned(),
            name: Some("test-runner".to_owned()),
            confinement: herd_runner::Confinement::with_data_root(data_root.path()),
            switch_file: Some(switch_file.clone()),
            // Fast, so that a runner which was going to leave has left long
            // before this test stops waiting for it.
            switch_poll: Duration::from_millis(50),
            ..herd_runner::Config::default()
        },
        // Nothing ever asks it to stop; only the switch could make it leave.
        std::future::pending::<String>(),
    ));

    let left = tokio::time::timeout(Duration::from_secs(2), runner).await;
    assert!(
        left.is_err(),
        "a runner started on a persisted drain must not leave: it did, with {left:?}"
    );

    // And the file agrees, so there is one state and not two: the hub is
    // being told `off`, and the next restart reads `off` rather than doing
    // this conversion again.
    assert_eq!(
        herd_runner::switch::read(&switch_file),
        Ok(Some(Switch::Off {
            reason: "the rig is mine tonight".to_owned()
        })),
        "the conversion is written back, with the reason its owner gave"
    );
}

/// The position that is *meant* to survive a restart still does, and this is
/// the control: the fix above must not have turned `off` into something a
/// runner argues with.
#[tokio::test(flavor = "multi_thread")]
async fn a_runner_started_on_a_persisted_off_stays_off_and_stays_up() {
    let home = TempDir::new("persisted-off");
    let switch_file = herd_runner::switch::default_path(home.path());
    let off = Switch::Off {
        reason: "she is gaming".to_owned(),
    };
    herd_runner::switch::write(&switch_file, &off).expect("the fixture writes the position");

    let data_root = TempDir::new("persisted-off-root");
    let runner = tokio::spawn(herd_runner::run_until(
        herd_runner::Config {
            hub_url: "ws://127.0.0.1:1/herd".to_owned(),
            name: Some("test-runner".to_owned()),
            confinement: herd_runner::Confinement::with_data_root(data_root.path()),
            switch_file: Some(switch_file.clone()),
            switch_poll: Duration::from_millis(50),
            ..herd_runner::Config::default()
        },
        std::future::pending::<String>(),
    ));

    let left = tokio::time::timeout(Duration::from_secs(1), runner).await;
    assert!(
        left.is_err(),
        "a runner switched off stays connected so that it can be switched on again: {left:?}"
    );
    assert_eq!(
        herd_runner::switch::read(&switch_file),
        Ok(Some(off)),
        "and nothing rewrote her file"
    );
}
