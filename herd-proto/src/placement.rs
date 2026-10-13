//! How big a runner is, and the parallelism one build slot gets on it (E-22).
//!
//! Two things live here because both sides need the same arithmetic
//! (invariant 4): the figures a runner announces about its host, and the
//! derivation the hub makes from them. A runner that computed its own `-j`
//! and a hub that computed another would be two answers to one question, and
//! the one on the job's record would be the one nobody ran.
//!
//! **Why a derivation at all.** The first consumer's build takes `-j N`, and
//! the right N is not a property of the job: it is a property of the machine
//! the job lands on. They measure **1 792 MiB per build thread**, so the same
//! `cargo xtask pack` wants `-j 7` on a 17 GiB laptop and `-j 6` in each of
//! two slots on a 31 GiB server. Nobody can write that in a submission
//! without naming a machine (invariant 6), so the hub derives it at the
//! moment it chooses one, records the figure, and passes that same figure to
//! the job.

use serde::{Deserialize, Serialize};

/// How much of a host's memory may go to build threads, as a fraction.
///
/// Three quarters. The last quarter is the host's own: the page cache the
/// build reads its inputs through, the linker's peak over the mean the
/// per-thread figure is, the desktop session of whoever owns the machine. A
/// runner must never be the reason its host falls over (invariant 2), and the
/// per-thread figure the consumer measured — 1 792 MiB — is an average over a
/// build, not a ceiling.
///
/// It also reproduces their own measured answer: three quarters of 17 GiB is
/// 13 056 MiB, which is 7 threads of 1 792 MiB, and `-j 7` on 17 GiB is
/// exactly the figure E-22 asks to see. A rule that handed out all 17 GiB
/// would say 9, which is two threads past what they found fits.
pub const BUILD_SHARE_NUMERATOR: u64 = 3;
/// The denominator of [`BUILD_SHARE_NUMERATOR`].
pub const BUILD_SHARE_DENOMINATOR: u64 = 4;

/// What a runner says about the host it runs on, in its handshake.
///
/// Both figures are optional and **a figure a host does not expose stays
/// absent, never zero** (D23's rule, and the defect it was written for): a
/// host reporting 0 MiB would have a `-j` derived from nothing. A runner that
/// announces neither figure is a runner no derivation can be made for, and
/// the hub says so by name rather than guessing one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSize {
    /// How much memory this host has for Herd's jobs, in MiB.
    ///
    /// What the *project* may use, not what `free` prints: on the first
    /// consumer's PC that is "~17 GiB usable by the project" out of 24 GB
    /// given to WSL. Read from the host, or given at startup by the owner,
    /// who is the authority on how much of her machine she is lending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<u64>,
    /// How many hardware threads this host has for Herd's jobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<u32>,
}

impl HostSize {
    pub fn new(memory_mib: Option<u64>, threads: Option<u32>) -> Self {
        Self {
            memory_mib,
            threads,
        }
    }

    pub fn is_silent(&self) -> bool {
        self.memory_mib.is_none() && self.threads.is_none()
    }

    /// Which figure is missing, as a sentence, or `None` when both are there.
    pub fn what_is_missing(&self) -> Option<&'static str> {
        match (self.memory_mib, self.threads) {
            (Some(_), Some(_)) => None,
            (None, Some(_)) => Some("how much memory it has"),
            (Some(_), None) => Some("how many threads it has"),
            (None, None) => Some("how much memory or how many threads it has"),
        }
    }

    /// The derivation for one slot, or the reason there cannot be one.
    ///
    /// `slots` is how many jobs may hold that slot at once on this host: the
    /// share is divided by it, because invariant 2 is a promise about the host
    /// with **every** slot full, not about the first job to arrive. Two build
    /// slots on a 31 GiB server are therefore `-j 6` each and not `-j 12`
    /// each.
    pub fn parallelism(&self, build_thread_mib: u32, slots: u32) -> Result<Parallelism, String> {
        let (Some(memory_mib), Some(threads)) = (self.memory_mib, self.threads) else {
            return Err(format!(
                "it does not say {}, so a -j cannot be derived for it",
                self.what_is_missing().unwrap_or("how big it is")
            ));
        };
        if build_thread_mib == 0 {
            return Err(
                "a build thread that needs 0 MiB is not a figure; a job asking for a derived \
                 -j has to say how much memory one of its threads needs"
                    .to_owned(),
            );
        }
        let slots = slots.max(1);
        let share = memory_mib * BUILD_SHARE_NUMERATOR / BUILD_SHARE_DENOMINATOR / u64::from(slots);
        // Rounded **down**, always. A thread the memory does not cover is a
        // host that swaps, and a host that swaps is invariant 2 broken; a
        // thread fewer than it could have had costs minutes.
        let from_memory = u32::try_from(share / u64::from(build_thread_mib)).unwrap_or(u32::MAX);
        let from_threads = (threads / slots).max(1);
        let wanted = from_memory.min(from_threads);
        Ok(Parallelism {
            // Never zero: `-j 0` is either "no limit" or an error depending on
            // the tool, and both are worse than one thread. A slot whose share
            // does not cover a single thread says so in `fits`, and the figure
            // stays 1.
            threads: wanted.max(1),
            fits: from_memory >= 1,
            memory_mib,
            host_threads: threads,
            build_thread_mib,
            slots,
            share_mib: share,
            from_memory,
            from_threads,
        })
    }
}

/// A derived `-j`, with every figure it was derived from.
///
/// The figures ride along because this ends up on a job's record as the one
/// line explaining its placement (E-36), and a figure with no arithmetic
/// behind it is something an operator can only believe or disbelieve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parallelism {
    /// The figure the job is actually given. Never zero.
    pub threads: u32,
    /// Whether the slot's share of memory covers even one build thread.
    /// False means the figure is the floor of one rather than a fit.
    pub fits: bool,
    pub memory_mib: u64,
    pub host_threads: u32,
    pub build_thread_mib: u32,
    pub slots: u32,
    /// The slot's share of the host's memory, in MiB.
    pub share_mib: u64,
    /// How many threads that share covers.
    pub from_memory: u32,
    /// How many threads the host's own threads allow this slot.
    pub from_threads: u32,
}

impl Parallelism {
    /// The one sentence a human reads, with the whole arithmetic in it.
    pub fn sentence(&self) -> String {
        let mut said = format!(
            "-j {} from {} MiB and {} threads, {} MiB for this slot ({} of {}), \
             {} MiB per build thread: memory allows {}, threads allow {}",
            self.threads,
            self.memory_mib,
            self.host_threads,
            self.share_mib,
            BUILD_SHARE_NUMERATOR,
            BUILD_SHARE_DENOMINATOR,
            self.build_thread_mib,
            self.from_memory,
            self.from_threads,
        );
        if self.slots > 1 {
            said.push_str(&format!(", shared between {} slots", self.slots));
        }
        if !self.fits {
            said.push_str(&format!(
                " — which does not cover one thread of {} MiB, so this job gets -j 1 and no \
                 less",
                self.build_thread_mib
            ));
        }
        said
    }
}

/// The token a command carries where its derived `-j` goes (E-22).
///
/// One token, chosen here rather than left to a convention, because the
/// figure on the record has to be the figure the job ran with: a job that
/// asks for a derivation and has nowhere to put it would be recorded as
/// `-j 7` and run with whatever its own default is. A job declaring a
/// per-thread figure must carry exactly one of these, and is refused by name
/// if it does not.
///
/// The wider substitution contract — `{job}`, `{commit}`, `{in}`, `{out}` —
/// is the command contract's, in M10. This is not that: it is one token, and
/// it exists because E-22's figure is meaningless unless it reaches the
/// process.
pub const PARALLELISM_TOKEN: &str = "{j}";

/// How many times the token appears in a command line.
pub fn parallelism_tokens(command: &[String]) -> usize {
    command
        .iter()
        .map(|word| word.matches(PARALLELISM_TOKEN).count())
        .sum()
}

/// The command with its `{j}` replaced by `threads`.
pub fn with_parallelism(command: &[String], threads: u32) -> Vec<String> {
    command
        .iter()
        .map(|word| word.replace(PARALLELISM_TOKEN, &threads.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// E-22's own figure, from the first consumer's own measurement: 17 GiB
    /// and 1 792 MiB per build thread is `-j 7`.
    #[test]
    fn the_owner_s_pc_derives_the_figure_e22_asks_for() {
        let pc = HostSize::new(Some(17 * 1024), Some(20));
        let derived = pc.parallelism(1792, 1).expect("both figures are there");
        assert_eq!(derived.threads, 7);
        assert!(derived.fits);
        let said = derived.sentence();
        assert!(said.contains("-j 7"), "{said}");
        assert!(said.contains("17408 MiB"), "{said}");
        assert!(said.contains("1792 MiB per build thread"), "{said}");
    }

    /// Two slots halve the share: a host must survive both of them being
    /// full, not just the first.
    #[test]
    fn two_build_slots_each_get_half_the_share() {
        let server = HostSize::new(Some(31 * 1024), Some(12));
        assert_eq!(
            server.parallelism(1792, 2).expect("sized").threads,
            6,
            "31 GiB in two slots is six threads each"
        );
        assert_eq!(
            server.parallelism(1792, 1).expect("sized").threads,
            12,
            "one slot is capped by the twelve threads it has"
        );
    }

    /// Rounding up would hand out a thread the memory does not cover, and the
    /// host would swap: invariant 2. Rounding down is the only direction.
    #[test]
    fn the_figure_is_rounded_down_and_never_up() {
        // Three quarters of 8 192 is 6 144, which is 3.43 threads of 1 792.
        let host = HostSize::new(Some(8192), Some(16));
        assert_eq!(host.parallelism(1792, 1).expect("sized").threads, 3);
    }

    /// And rounding down must never reach zero, whatever the figures are:
    /// `-j 0` is "no limit" to make and an error to cargo, and a job that
    /// cannot run is worse than a job that runs slowly.
    #[test]
    fn a_slot_too_small_for_one_thread_still_gets_one_and_says_so() {
        let tiny = HostSize::new(Some(512), Some(4));
        let derived = tiny.parallelism(1792, 1).expect("sized");
        assert_eq!(derived.threads, 1);
        assert!(!derived.fits);
        assert!(derived.sentence().contains("does not cover one thread"));
        // Nor through the slot divisor.
        let shared = HostSize::new(Some(4096), Some(8))
            .parallelism(1792, 8)
            .expect("sized");
        assert_eq!(shared.threads, 1);
    }

    /// A host that does not say how big it is gets no derivation at all. The
    /// absent figure is named; nothing is invented for it, and above all it
    /// is not read as zero.
    #[test]
    fn an_unsized_host_is_refused_by_name_rather_than_read_as_zero() {
        let silent = HostSize::default();
        let why = silent.parallelism(1792, 1).expect_err("nothing to go on");
        assert!(why.contains("how much memory or how many threads"), "{why}");
        let half = HostSize::new(Some(17408), None);
        let why = half.parallelism(1792, 1).expect_err("no thread count");
        assert!(why.contains("how many threads"), "{why}");
        let other_half = HostSize::new(None, Some(20));
        let why = other_half.parallelism(1792, 1).expect_err("no memory");
        assert!(why.contains("how much memory"), "{why}");
    }

    #[test]
    fn a_per_thread_figure_of_zero_is_not_a_figure() {
        let why = HostSize::new(Some(17408), Some(20))
            .parallelism(0, 1)
            .expect_err("zero is not a figure");
        assert!(why.contains("0 MiB is not a figure"), "{why}");
    }

    #[test]
    fn the_token_is_counted_and_replaced_wherever_it_sits() {
        let command = vec![
            "cargo".to_owned(),
            "xtask".to_owned(),
            "test".to_owned(),
            "-j".to_owned(),
            "{j}".to_owned(),
        ];
        assert_eq!(parallelism_tokens(&command), 1);
        assert_eq!(with_parallelism(&command, 7)[4], "7");
        // Inside a word, as a shell line carries it.
        let line = vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "make -j{j}".to_owned(),
        ];
        assert_eq!(parallelism_tokens(&line), 1);
        assert_eq!(with_parallelism(&line, 6)[2], "make -j6");
        // And a command that carries none, or two.
        assert_eq!(parallelism_tokens(&["true".to_owned()]), 0);
        assert_eq!(parallelism_tokens(&["{j} {j}".to_owned()]), 2);
    }
}
