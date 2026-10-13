//! What the hub knows about a runner's capabilities.
//!
//! The runner is the authority: it announces what it offers in its handshake,
//! with each capability's origin (E-24), and announces what its external
//! probes say whenever one of them changes its mind (E-26). The hub records
//! both, and the only judgement it makes with them in M5 is the small one —
//! whether a job's labels can be given to this runner *now*, and if not, the
//! sentence that goes on the job's record.
//!
//! What is deliberately not here. The algebra itself lives in `herd-proto`
//! (invariant 4), including [`herd_proto::Holdings`], which decides contention
//! between jobs already running. Consulting it is the claim step, and the
//! claim step is M7's: matching, counted build slots and stickiness all land
//! there, on top of this. M5 stops at "offered, and not suspended", which is
//! exactly what a busy probe needs in order to keep a job queued.
//!
//! The reason a job waits is never a refusal. `Refused` is terminal (D13) and
//! a suspended capability is the opposite of terminal: the probe clears, the
//! job runs, and nothing was ever failed (D38, E-26).

use std::collections::BTreeMap;

use herd_proto::{Availability, CapabilityOffer, CapabilityState, LabelSet};

/// What a suspension reads as when the reading that suspended it carried no
/// reason. Every [`Availability`] that suspends has one today; this is here so
/// that the hub never has to invent one, and never has to treat "no reason" as
/// "not suspended".
const UNSAID: &str = "its probe did not say why";

/// Why a runner cannot be given a job right now, and whether waiting here
/// would help (E-19, E-36).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldUp {
    /// True when the obstacle is the moment — a probe outside Herd holding a
    /// capability — and false when it is the machine: a label it does not
    /// offer, or one its owner denied.
    pub clears_with_time: bool,
    /// The one sentence, with the probe's own words in it.
    pub reason: String,
}

/// One connected runner's offer, and what its probes last said.
#[derive(Debug, Clone, Default)]
pub struct RunnerCapacity {
    /// The name the runner gave in its handshake, for sentences a human
    /// reads. A runner that gave none is "the runner": there is one in M5.
    name: Option<String>,
    offer: CapabilityOffer,
    /// Only the capabilities that have a probe. A capability with no probe is
    /// absent here and read as [`Availability::Free`], which is what it is.
    availability: BTreeMap<String, Availability>,
}

impl RunnerCapacity {
    pub fn new(name: Option<String>, offer: CapabilityOffer) -> Self {
        Self {
            name,
            offer,
            availability: BTreeMap::new(),
        }
    }

    /// How this runner is named in a sentence.
    pub fn named(&self) -> &str {
        self.name.as_deref().unwrap_or("the runner")
    }

    pub fn offer(&self) -> &CapabilityOffer {
        &self.offer
    }

    /// Take a runner's snapshot of what its probes say.
    ///
    /// A whole snapshot replaces the previous one rather than merging into it:
    /// the runner sends every probed capability each time, so there is nothing
    /// to reconcile and a frame nobody saw costs one interval.
    pub fn update(&mut self, states: Vec<CapabilityState>) {
        self.availability = states
            .into_iter()
            .map(|state| (state.name, state.availability))
            .collect();
    }

    /// What this runner's probe says about one capability.
    ///
    /// **A capability that has a probe and no reading is suspended.** This is
    /// the join race, and it is the one place the hub has to make the same
    /// judgement the runner makes: a runner is registered the instant its
    /// handshake lands, one frame before the snapshot of its probes arrives,
    /// and a hub that read "no reading" as "free" would dispatch into that
    /// window — handing out a capability something outside Herd holds, which
    /// is exactly what E-26 forbids. The offer itself says which capabilities
    /// have a probe, so the hub needs nothing else to know it is waiting for a
    /// reading.
    ///
    /// A capability with no probe is free, because there is nothing to ask.
    pub fn availability_of(&self, label: &str) -> Availability {
        if let Some(read) = self.availability.get(label) {
            return read.clone();
        }
        match self
            .offer
            .get(label)
            .and_then(|entry| entry.capability.busy_probe.as_ref())
        {
            Some(probe) => Availability::unknown(format!(
                "{} has a busy probe ({}) and {} has not reported a reading of \
                 it yet",
                label,
                probe.describe(),
                self.named()
            )),
            None => Availability::Free,
        }
    }

    /// Every capability currently suspended, with its reason: one a probe
    /// says is busy, one whose probe could not be read, and one whose reading
    /// has not arrived yet.
    ///
    /// Whether a reading suspends is [`Availability::suspends`]'s judgement
    /// and nothing else's, here as in [`Self::why_not_now`]: the hub asks the
    /// question, it does not answer it.
    pub fn suspended(&self) -> Vec<(String, String)> {
        self.offer
            .entries()
            .iter()
            .filter(|entry| entry.capability.busy_probe.is_some())
            .filter_map(|entry| {
                let state = self.availability_of(entry.name());
                if !state.suspends() {
                    return None;
                }
                Some((
                    entry.name().to_owned(),
                    state.reason().unwrap_or(UNSAID).to_owned(),
                ))
            })
            .collect()
    }

    /// Why this runner cannot be given a job needing `labels` right now, or
    /// `None` when it can.
    ///
    /// One sentence, built in one place, because it is what lands on the job's
    /// record as the reason it is waiting and what a test reads back. It names
    /// the label, the runner and the figure a probe gave, because a wait with
    /// no figures is not an explanation (E-04, which M8 completes).
    ///
    /// Two reasons in M5, and the difference between them matters to M7, not
    /// here: the label is not offered at all, or the label is offered and
    /// suspended. Neither is a refusal.
    pub fn why_not_now(&self, labels: &LabelSet) -> Option<String> {
        self.held_up_by(labels).map(|held_up| held_up.reason)
    }

    /// The same judgement, with M7's question answered too: does waiting
    /// *here* clear it?
    ///
    /// The two reasons M5 found are the two sides of E-36, and telling them
    /// apart is what M7's claim step is built on. A label this runner does
    /// not offer — or whose owner denied it — is not about the moment at all:
    /// no job ending here will make this machine offer it, so a job must not
    /// stick to this runner for it (E-19). A label something outside Herd is
    /// holding is exactly about the moment: the lock file goes, the probe
    /// clears, the job runs (D48).
    ///
    /// Neither is a refusal, here or in the hub (D38). What differs is only
    /// which machine the job waits *for*.
    pub fn held_up_by(&self, labels: &LabelSet) -> Option<HeldUp> {
        for label in labels.iter() {
            if self.offer.available(label).is_none() {
                let denied = self
                    .offer
                    .get(label)
                    .is_some_and(|entry| entry.origins.denied);
                return Some(HeldUp {
                    clears_with_time: false,
                    reason: if denied {
                        format!(
                            "{} has {label} and its owner has denied it, so this job \
                             waits for a runner that offers it",
                            self.named()
                        )
                    } else {
                        format!(
                            "{} does not offer {label}, so this job waits for a \
                             runner that does",
                            self.named()
                        )
                    },
                });
            }
            // Whether a reading suspends is `Availability::suspends`'s to
            // say — one judgement, made in `herd-proto`, for the runner that
            // reads a probe and the hub that acts on it alike (D48). The hub
            // reads the *reason* here, never decides from it: a reason is a
            // sentence for a human, and a hub that treated "has a reason" as
            // the test would be making the judgement a second time, in a
            // second place, where it could drift.
            let availability = self.availability_of(label);
            if availability.suspends() {
                let reason = availability.reason().unwrap_or(UNSAID);
                return Some(HeldUp {
                    clears_with_time: true,
                    reason: format!(
                        "{label} is suspended on {}: {reason}. This job waits and \
                         runs when the probe clears; it is not refused",
                        self.named(),
                    ),
                });
            }
        }
        None
    }

    /// What the hub says out loud when a runner joins: the offer as E-24 asks
    /// to see it, and anything a probe is already suspending.
    pub fn report(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for line in self.offer.report() {
            lines.push(format!("{} offers {line}", self.named()));
        }
        for (name, reason) in self.suspended() {
            lines.push(format!("{name} is suspended on {}: {reason}", self.named()));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herd_proto::{BusyProbe, Capability, CapabilityOrigins, OfferedCapability};

    fn offer(entries: Vec<OfferedCapability>) -> CapabilityOffer {
        CapabilityOffer::new(entries).expect("a valid offer")
    }

    fn labels(names: &[&str]) -> LabelSet {
        LabelSet::new(names.iter().map(|n| (*n).to_owned())).expect("valid labels")
    }

    fn capacity() -> RunnerCapacity {
        RunnerCapacity::new(
            Some("the-pc".to_owned()),
            offer(vec![
                OfferedCapability::new(
                    Capability::counted("build", 2),
                    CapabilityOrigins::declared(),
                ),
                OfferedCapability::new(
                    Capability::lease("metal-pc", BusyProbe::lock_file("/run/metal-pc")),
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
            ]),
        )
    }

    #[test]
    fn a_job_needing_nothing_is_never_held_up() {
        assert_eq!(capacity().why_not_now(&LabelSet::default()), None);
    }

    #[test]
    fn an_offered_and_unsuspended_label_is_given_at_once() {
        assert_eq!(capacity().why_not_now(&labels(&["build"])), None);
    }

    /// The join race. A runner is registered the frame before its probe
    /// snapshot arrives, and a probed capability with no reading must be
    /// suspended for that window rather than handed out.
    #[test]
    fn a_probed_capability_waits_for_its_first_reading() {
        let fresh = capacity();
        let reason = fresh
            .why_not_now(&labels(&["metal-pc"]))
            .expect("nothing has read the probe yet");
        assert!(reason.contains("has not reported a reading"), "{reason}");
        // While a capability with no probe is free from the first moment.
        assert_eq!(fresh.why_not_now(&labels(&["build"])), None);
    }

    #[test]
    fn a_denied_label_reads_as_denied_rather_than_as_absent() {
        let reason = capacity()
            .why_not_now(&labels(&["kvm"]))
            .expect("kvm is denied here");
        assert!(reason.contains("denied"), "{reason}");
        assert!(reason.contains("the-pc"), "{reason}");
    }

    #[test]
    fn a_label_nothing_offers_says_so_and_still_waits() {
        let reason = capacity()
            .why_not_now(&labels(&["nothing-offers-this"]))
            .expect("not offered");
        assert!(reason.contains("does not offer"), "{reason}");
        assert!(
            reason.contains("waits"),
            "a wait is never a refusal: {reason}"
        );
    }

    /// E-26 and D38 in one assertion: a busy probe suspends, the reason
    /// carries the probe's own words, and the sentence says the job waits.
    #[test]
    fn a_busy_probe_suspends_the_label_and_names_why() {
        let mut capacity = capacity();
        capacity.update(vec![CapabilityState::new("metal-pc", Availability::Free)]);
        assert_eq!(capacity.why_not_now(&labels(&["metal-pc"])), None);
        capacity.update(vec![CapabilityState::new(
            "metal-pc",
            Availability::busy("the lock file /run/metal-pc is there"),
        )]);
        let reason = capacity
            .why_not_now(&labels(&["metal-pc"]))
            .expect("suspended");
        assert!(reason.contains("suspended on the-pc"), "{reason}");
        assert!(reason.contains("/run/metal-pc"), "{reason}");
        assert!(reason.contains("not refused"), "{reason}");
        // And a label whose probe says nothing is unaffected.
        assert_eq!(capacity.why_not_now(&labels(&["build"])), None);
        // Cleared, the job is no longer held up.
        capacity.update(vec![CapabilityState::new("metal-pc", Availability::Free)]);
        assert_eq!(capacity.why_not_now(&labels(&["metal-pc"])), None);
    }

    /// A probe that could not be read suspends exactly as a busy one does —
    /// the judgement is `Availability::suspends`, made once in herd-proto —
    /// and the hub repeats the runner's reason rather than inventing one.
    #[test]
    fn an_unreadable_probe_suspends_too() {
        let mut capacity = capacity();
        capacity.update(vec![CapabilityState::new(
            "metal-pc",
            Availability::unknown("the probe /usr/bin/leased cannot be started"),
        )]);
        let reason = capacity
            .why_not_now(&labels(&["metal-pc"]))
            .expect("an unreadable probe suspends");
        assert!(reason.contains("cannot be started"), "{reason}");
    }

    /// A snapshot replaces rather than merges, and a probed capability that
    /// has dropped out of one is back to having no reading — suspended, never
    /// quietly free.
    #[test]
    fn a_snapshot_replaces_the_one_before_it() {
        let mut capacity = capacity();
        capacity.update(vec![CapabilityState::new("metal-pc", Availability::Free)]);
        assert_eq!(capacity.why_not_now(&labels(&["metal-pc"])), None);
        capacity.update(Vec::new());
        let reason = capacity
            .why_not_now(&labels(&["metal-pc"]))
            .expect("no reading again");
        assert!(reason.contains("has not reported a reading"), "{reason}");
    }

    #[test]
    fn what_the_hub_says_when_a_runner_joins_carries_every_origin() {
        let mut capacity = capacity();
        capacity.update(vec![CapabilityState::new(
            "metal-pc",
            Availability::busy("the lock file /run/metal-pc is there"),
        )]);
        let said = capacity.report().join("\n");
        assert!(
            said.contains("the-pc offers kvm: present, denied"),
            "{said}"
        );
        assert!(said.contains("the-pc offers build: declared"), "{said}");
        assert!(said.contains("metal-pc is suspended on the-pc"), "{said}");
    }
}
