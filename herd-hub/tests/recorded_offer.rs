//! What the hub recorded about the runner that joined it (M5, E-24).
//!
//! In process, because what is asserted is the hub's own state rather than a
//! line in a log: the offer it holds, the probe readings it was sent, and the
//! fact that it waits for a reading rather than assuming a probed capability
//! is free. M13's status document is built out of exactly this, and the
//! real-binary half of the same promise is in `capabilities.rs`.

use std::time::Duration;

use herd_proto::{
    BusyProbe, Capability, CapabilityOffer, CapabilityOrigins, LabelSet, OfferedCapability,
};
use herd_runner::capability::ProbeSettings;

mod support;

use support::{wait_until, Harness};

/// A runner that offers a counted capability, a lease with a lock-file probe,
/// a shared one and a denied one — one of every shape M5 has.
fn an_offer(lock: &std::path::Path) -> CapabilityOffer {
    CapabilityOffer::new(vec![
        OfferedCapability::new(
            Capability::counted("build", 2)
                .excluding(["nightly"])
                .expect("a valid exclusion"),
            CapabilityOrigins::declared(),
        ),
        OfferedCapability::new(
            Capability::lease(
                "metal-pc",
                BusyProbe::lock_file(lock.to_string_lossy().into_owned()),
            ),
            CapabilityOrigins::declared(),
        ),
        OfferedCapability::new(
            Capability::shared("reference"),
            CapabilityOrigins::declared(),
        ),
        OfferedCapability::new(
            Capability::shared("kvm"),
            CapabilityOrigins {
                probed: true,
                declared: false,
                denied: true,
            },
        ),
    ])
    .expect("a valid offer")
}

fn labels(names: &[&str]) -> LabelSet {
    LabelSet::new(names.iter().map(|n| (*n).to_owned())).expect("valid labels")
}

/// The hub holds the offer it was handed, unchanged and with its origins, and
/// it holds what the runner's probes said.
#[tokio::test]
async fn the_hub_records_the_offer_it_was_handed_and_the_readings_that_follow() {
    let run = herd_runner::testing::TempDir::new("recorded-offer");
    let lock = run.path().join("metal-pc.lease");
    std::fs::write(&lock, b"held outside herd").expect("the lease is held");
    let offer = an_offer(&lock);
    let harness = Harness::with_hub_and_runner_offering(
        offer.clone(),
        ProbeSettings {
            interval: Duration::from_millis(50),
            timeout: Duration::from_secs(2),
        },
    )
    .await;

    let held = harness.runner_capacity().expect("a runner has joined");
    assert_eq!(
        held.offer(),
        &offer,
        "the hub must hold the offer it was handed, not a reconstruction of it"
    );
    // Including what was denied: E-24's whole point is that it stays visible.
    assert_eq!(
        held.offer().report(),
        vec![
            "build: declared".to_owned(),
            "kvm: present, denied".to_owned(),
            "metal-pc: declared".to_owned(),
            "reference: declared".to_owned(),
        ]
    );
    assert_eq!(held.named(), "test-runner");

    // The lease's probe says busy, so the hub suspends it — and says so with
    // the runner's own words, never an invented reason.
    wait_until("the hub has been told the lease is held", || {
        harness
            .runner_capacity()
            .expect("still joined")
            .why_not_now(&labels(&["metal-pc"]))
            .is_some_and(|why| why.contains("lease"))
    })
    .await;
    let held = harness.runner_capacity().expect("still joined");
    assert_eq!(
        held.suspended().len(),
        1,
        "only the probed capability is suspended: {:?}",
        held.suspended()
    );
    assert_eq!(held.suspended()[0].0, "metal-pc");
    // Everything else is free, denial aside.
    assert_eq!(held.why_not_now(&labels(&["build", "reference"])), None);
    assert!(held
        .why_not_now(&labels(&["kvm"]))
        .expect("denied")
        .contains("denied"));

    // The lease is given back, and the hub is told without anyone asking.
    std::fs::remove_file(&lock).expect("the lease is released");
    wait_until("the hub has been told the lease is free", || {
        harness
            .runner_capacity()
            .expect("still joined")
            .why_not_now(&labels(&["metal-pc"]))
            .is_none()
    })
    .await;
    assert!(harness
        .runner_capacity()
        .expect("still joined")
        .suspended()
        .is_empty());
}

/// A runner that offers nothing is still a runner: it joins, the hub records
/// an empty offer, and a job that needs no capability runs on it.
#[tokio::test]
async fn a_runner_that_offers_nothing_still_runs_a_job_that_needs_nothing() {
    let harness =
        Harness::with_hub_and_runner_offering(CapabilityOffer::empty(), ProbeSettings::default())
            .await;
    let held = harness.runner_capacity().expect("a runner has joined");
    assert!(held.offer().is_empty());
    assert!(held.report().is_empty());
    assert_eq!(held.why_not_now(&LabelSet::default()), None);
    let finished = support::run_job(&harness.requester(), support::shell("exit 0"), |_, _| {})
        .await
        .expect("the job runs");
    assert_eq!(finished.outcome, herd_proto::JobOutcome::Exited { code: 0 });
}
