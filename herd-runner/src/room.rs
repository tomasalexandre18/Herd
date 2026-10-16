//! How much memory there really is where this runner is standing (D119).
//!
//! The figure a runner announces is what the hub admits against, and until
//! now it was `MemTotal`: the whole machine. A runner started inside a
//! systemd slice that carries `MemoryMax` — which is how the first consumer
//! runs things, and how `deploy/herd-runner.service` runs ours — has **less**
//! room than the machine advertises, and admitting against the machine is how
//! a slice gets OOM-killed while Herd believes it was careful.
//!
//! So the room is read from the cgroup the runner is **actually in**: walk
//! from its own cgroup to the root, take the tightest ceiling found on the
//! way, and subtract what is already charged there. Where nothing up the
//! chain carries a ceiling there is nothing to read and the host figure
//! stands untouched — the stricter read costs nothing on a bare machine,
//! which is the whole point of doing it this way rather than by option.
//!
//! **Which knob is the ceiling: the lower of `memory.max` and `memory.high`.**
//! They fail differently — past `memory.max` the kernel OOM-kills the cgroup,
//! past `memory.high` it throttles the allocator and reclaims hard — but from
//! admission's point of view they are the same statement: this is how much
//! memory the jobs here can hold before something goes wrong. A job that
//! crawls under reclaim pressure is indistinguishable from a hung job and
//! will reach its time cap instead of its memory cap, so a `memory.high`
//! below `memory.max` is a real bound and is treated as one. Taking the lower
//! of the two is the only figure that is true whichever one is set.
//!
//! What is **not** read here is `MemAvailable`, which the first consumer
//! asked for. The hub's arithmetic subtracts the floor (E-29) and what the
//! jobs it believes are running have reserved; a second figure that moves
//! between the judgement and the claim would be the overcommit E-32 forbids.
//! `memory.current` is subtracted once, at startup, for the same reason the
//! tmpfs figure is a capacity and not what is free this second (D79).

use std::fs;
use std::path::{Path, PathBuf};

/// Where cgroup v2 is mounted on every host Herd runs on.
pub const CGROUP_MOUNT: &str = "/sys/fs/cgroup";

/// Where the kernel says which cgroup this process is in.
const SELF_CGROUP: &str = "/proc/self/cgroup";

/// The tightest memory ceiling enclosing this runner, and the room under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enclosure {
    /// The cgroup that carries the ceiling, written as the kernel writes it:
    /// `/user.slice/herd.slice`. Named so the startup line says *where* the
    /// bound comes from, which is the file an operator has to go and edit.
    pub cgroup: String,
    /// `memory.max` or `memory.high`, whichever was lower.
    pub knob: &'static str,
    /// The ceiling itself, in bytes.
    pub ceiling_bytes: u64,
    /// What that cgroup had already charged when this was read.
    pub charged_bytes: u64,
}

impl Enclosure {
    /// The room left under the ceiling, in MiB.
    ///
    /// Saturating: a cgroup already over its `memory.high` is a cgroup with
    /// no room, which is a figure, and the floor refusal in `main` is what
    /// turns "no room" into a runner that says so instead of starting.
    pub fn room_mib(&self) -> u64 {
        self.ceiling_bytes.saturating_sub(self.charged_bytes) / (1 << 20)
    }

    /// The startup line, which has to name the cgroup, the knob and both
    /// figures: an operator reading a refusal must be able to find out why
    /// this machine announced less than it has.
    pub fn why(&self) -> String {
        format!(
            "{} is enclosed by {} {} on {}, which already holds {}, so the room here is {} \
             and not what /proc/meminfo reports",
            SELF_CGROUP,
            self.knob,
            herd_proto::size_mib(self.ceiling_bytes / (1 << 20)),
            self.cgroup,
            herd_proto::size_mib(self.charged_bytes / (1 << 20)),
            herd_proto::size_mib(self.room_mib())
        )
    }
}

/// The tightest memory ceiling on the way from `cgroup` to the root of
/// `mount`, or `None` when nothing up the chain carries one.
///
/// `mount` and `cgroup` are arguments rather than constants so that the walk
/// and the arithmetic are asserted against a tree a test builds, on any host:
/// the thing this host cannot do is *create* a bounded cgroup, not read one.
pub fn enclosing(mount: &Path, cgroup: &str) -> Option<Enclosure> {
    let mut tightest: Option<Enclosure> = None;
    let mut relative = PathBuf::from("/");
    for component in Path::new(cgroup)
        .components()
        .filter_map(|part| match part {
            std::path::Component::Normal(name) => Some(name),
            _ => None,
        })
        // The root of the mount is walked too: a ceiling can sit anywhere up
        // the chain, including on the whole hierarchy.
        .map(Some)
        .chain(std::iter::once(None))
    {
        let directory = mount.join(relative.strip_prefix("/").unwrap_or(&relative));
        if let Some(found) = ceiling_at(&directory, &relative) {
            if tightest
                .as_ref()
                .is_none_or(|known| found.ceiling_bytes < known.ceiling_bytes)
            {
                tightest = Some(found);
            }
        }
        match component {
            Some(name) => relative.push(name),
            None => break,
        }
    }
    tightest
}

/// The ceiling one cgroup directory carries, if it carries one.
fn ceiling_at(directory: &Path, cgroup: &Path) -> Option<Enclosure> {
    let mut lowest: Option<(&'static str, u64)> = None;
    for knob in ["memory.max", "memory.high"] {
        let Some(bytes) = limit_in(&directory.join(knob)) else {
            continue;
        };
        if lowest.is_none_or(|(_, known)| bytes < known) {
            lowest = Some((knob, bytes));
        }
    }
    let (knob, ceiling_bytes) = lowest?;
    Some(Enclosure {
        cgroup: cgroup.display().to_string(),
        knob,
        ceiling_bytes,
        // A cgroup with a ceiling always has `memory.current`; an unreadable
        // one is read as nothing charged, which announces *more* room than
        // there is — so it is read as the ceiling being fully charged
        // instead. Erring towards less room is the only direction invariant 2
        // allows.
        charged_bytes: charged_in(&directory.join("memory.current")).unwrap_or(ceiling_bytes),
    })
}

/// The figure in a `memory.max` or `memory.high`, or `None` for `max` — the
/// word the kernel writes for "no ceiling", which is not a figure.
fn limit_in(path: &Path) -> Option<u64> {
    let text = fs::read_to_string(path).ok()?;
    let word = text.trim();
    if word == "max" {
        return None;
    }
    word.parse().ok()
}

fn charged_in(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// This process's cgroup, from the `0::` line of `/proc/self/cgroup`.
///
/// Only that line: a host still showing cgroup v1 controllers writes one line
/// per hierarchy, and none of them is a v2 path. No `0::` line is a host with
/// no cgroup v2 at all, which is a host with no enclosing ceiling to read.
pub fn own_cgroup(proc_self_cgroup: &str) -> Option<String> {
    proc_self_cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(|path| path.trim().to_owned())
}

/// The memory figure this runner announces: what it was going to lend, less
/// whatever the cgroup it stands in will not let it have.
///
/// **Only ever lowers.** An enclosing ceiling above the figure is not news —
/// a 64 GiB slice on a 32 GiB machine bounds nothing — and raising the figure
/// to meet a ceiling would announce memory the machine does not have, which
/// is the defect [`crate::metrics::lendable_memory_mib`] exists to stop. So
/// this composes with the owner's `--host-memory` in one direction: she may
/// lend less than her machine has, and the kernel may lend her less still.
///
/// The second return is the line to print when the figure moved, naming both
/// figures and the cgroup: an operator reading a refusal has to be able to
/// find out why this machine announced less than `free` shows.
pub fn announced_memory_mib(
    lent_mib: Option<u64>,
    enclosure: Option<Enclosure>,
) -> (Option<u64>, Option<String>) {
    let Some(enclosure) = enclosure else {
        return (lent_mib, None);
    };
    let room = enclosure.room_mib();
    if lent_mib.is_some_and(|lent| lent <= room) {
        return (lent_mib, None);
    }
    (
        Some(room),
        Some(format!(
            "announcing {} of memory rather than {}, because {}",
            herd_proto::size_mib(room),
            lent_mib.map_or("an unread host figure".to_owned(), herd_proto::size_mib),
            enclosure.why()
        )),
    )
}

/// The room where this runner is standing, read from this host.
///
/// `None` is "nothing encloses this runner", and the caller then keeps the
/// host figure: on a bare machine this must change nothing at all.
pub fn measure() -> Option<Enclosure> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let own = own_cgroup(&fs::read_to_string(SELF_CGROUP).ok()?)?;
    enclosing(Path::new(CGROUP_MOUNT), &own)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    /// Build a cgroup-shaped tree: one directory per level, each with the
    /// files it is given.
    fn tree(root: &Path, levels: &[(&str, &[(&str, &str)])]) {
        let mut at = root.to_path_buf();
        for (name, files) in levels {
            if !name.is_empty() {
                at = at.join(name);
            }
            fs::create_dir_all(&at).expect("a directory under a temporary root");
            for (file, content) in *files {
                fs::write(at.join(file), content).expect("a writable file");
            }
        }
    }

    /// D119's figure: the tightest ceiling anywhere up the chain, less what
    /// that cgroup already holds. A runner three levels down from the slice
    /// that bounds it must still find the bound.
    #[test]
    fn the_tightest_ceiling_up_the_chain_wins_and_what_is_charged_is_subtracted() {
        let mount = TempDir::new("room-chain");
        tree(
            mount.path(),
            &[
                ("", &[("memory.max", "max"), ("memory.current", "0")]),
                (
                    "herd.slice",
                    &[
                        ("memory.max", &(8 * (1u64 << 30)).to_string()),
                        ("memory.current", &(1u64 << 30).to_string()),
                    ],
                ),
                (
                    "runner.service",
                    &[
                        ("memory.max", &(16 * (1u64 << 30)).to_string()),
                        ("memory.current", &(1u64 << 30).to_string()),
                    ],
                ),
            ],
        );
        let found = enclosing(mount.path(), "/herd.slice/runner.service")
            .expect("a slice with MemoryMax encloses this");
        assert_eq!(found.cgroup, "/herd.slice");
        assert_eq!(found.knob, "memory.max");
        assert_eq!(found.room_mib(), 7 * 1024, "8 GiB less the 1 GiB charged");
        let why = found.why();
        assert!(
            why.contains("/herd.slice") && why.contains("memory.max"),
            "{why}"
        );
        assert!(why.contains("7 GiB") && why.contains("8 GiB"), "{why}");
    }

    /// A `memory.high` under the `memory.max` is the real bound: past it the
    /// kernel throttles and reclaims, which a job meets as a time cap rather
    /// than as a memory cap. The lower of the two is the figure.
    #[test]
    fn a_memory_high_below_the_max_is_the_ceiling() {
        let mount = TempDir::new("room-high");
        tree(
            mount.path(),
            &[
                ("", &[]),
                (
                    "herd.slice",
                    &[
                        ("memory.max", &(8 * (1u64 << 30)).to_string()),
                        ("memory.high", &(4 * (1u64 << 30)).to_string()),
                        ("memory.current", "0"),
                    ],
                ),
            ],
        );
        let found = enclosing(mount.path(), "/herd.slice").expect("a ceiling is there");
        assert_eq!(found.knob, "memory.high");
        assert_eq!(found.room_mib(), 4 * 1024);
    }

    /// The bare machine, which is the case that must cost nothing: every
    /// `memory.max` up the chain reads `max`, so there is no figure to read
    /// and the caller keeps `MemTotal`.
    #[test]
    fn a_chain_of_unlimited_cgroups_encloses_nothing() {
        let mount = TempDir::new("room-bare");
        tree(
            mount.path(),
            &[
                ("", &[]),
                (
                    "user.slice",
                    &[
                        ("memory.max", "max"),
                        ("memory.high", "max"),
                        ("memory.current", "1864634368"),
                    ],
                ),
                (
                    "session-1.scope",
                    &[("memory.max", "max"), ("memory.current", "17")],
                ),
            ],
        );
        assert_eq!(enclosing(mount.path(), "/user.slice/session-1.scope"), None);
        // And a tree with no memory controller at all, which is a host where
        // the question cannot be asked rather than one with a huge ceiling.
        let empty = TempDir::new("room-nocontroller");
        tree(empty.path(), &[("", &[]), ("some.slice", &[])]);
        assert_eq!(enclosing(empty.path(), "/some.slice"), None);
    }

    /// A cgroup whose ceiling reads but whose `memory.current` does not is
    /// read as fully charged. Announcing the whole ceiling there would
    /// announce room that may not exist, and invariant 2 only lets this err
    /// towards less.
    #[test]
    fn an_unreadable_charge_is_read_as_no_room_left() {
        let mount = TempDir::new("room-nocurrent");
        tree(
            mount.path(),
            &[
                ("", &[]),
                ("herd.slice", &[("memory.max", &(1u64 << 30).to_string())]),
            ],
        );
        let found = enclosing(mount.path(), "/herd.slice").expect("the ceiling reads");
        assert_eq!(found.room_mib(), 0);
    }

    #[test]
    fn the_v2_line_is_the_only_one_that_names_a_cgroup() {
        assert_eq!(
            own_cgroup("0::/user.slice/user-1002.slice/session-67.scope\n").as_deref(),
            Some("/user.slice/user-1002.slice/session-67.scope")
        );
        // A hybrid host: v1 hierarchies first, the v2 line among them.
        assert_eq!(
            own_cgroup("8:memory:/system.slice/x\n1:name=systemd:/user.slice\n0::/herd\n")
                .as_deref(),
            Some("/herd")
        );
        // v1 only: no v2 path, so no ceiling to read.
        assert_eq!(own_cgroup("8:memory:/system.slice/x\n"), None);
        assert_eq!(own_cgroup(""), None);
    }

    fn enclosure_of(ceiling_gib: u64, charged_gib: u64) -> Enclosure {
        Enclosure {
            cgroup: "/herd.slice".to_owned(),
            knob: "memory.max",
            ceiling_bytes: ceiling_gib * (1 << 30),
            charged_bytes: charged_gib * (1 << 30),
        }
    }

    /// A tighter enclosing ceiling lowers the figure the hub admits against,
    /// and the line says which cgroup took it down.
    #[test]
    fn a_tighter_enclosing_ceiling_lowers_what_is_announced() {
        let (announced, said) = announced_memory_mib(Some(31 * 1024), Some(enclosure_of(8, 1)));
        assert_eq!(announced, Some(7 * 1024));
        let said = said.expect("a figure that moved is never silent");
        assert!(said.contains("7 GiB") && said.contains("31 GiB"), "{said}");
        assert!(said.contains("/herd.slice"), "{said}");
        // And with no host figure at all, the room is the figure.
        let (announced, said) = announced_memory_mib(None, Some(enclosure_of(4, 0)));
        assert_eq!(announced, Some(4 * 1024));
        assert!(said.expect("said").contains("an unread host figure"));
    }

    /// A ceiling **above** the figure bounds nothing, and raising the figure
    /// to meet it would announce memory the machine does not have — the one
    /// mistake `lendable_memory_mib` exists to stop. Unchanged and unremarked.
    #[test]
    fn an_enclosing_ceiling_never_raises_what_is_announced() {
        let (announced, said) = announced_memory_mib(Some(16 * 1024), Some(enclosure_of(64, 0)));
        assert_eq!(announced, Some(16 * 1024));
        assert_eq!(said, None, "a ceiling that bounds nothing is not news");
        // Exactly equal is not news either.
        let (announced, said) = announced_memory_mib(Some(8 * 1024), Some(enclosure_of(8, 0)));
        assert_eq!(announced, Some(8 * 1024));
        assert_eq!(said, None);
    }

    /// D119's other half, and the one that must cost nothing: **no**
    /// enclosing limit leaves the host figure exactly as it was. Asserted
    /// against this host and not a fixture, because "the stricter read is
    /// free on a bare machine" is a claim about a real `/proc/self/cgroup`.
    #[test]
    fn no_enclosing_limit_leaves_the_host_figure_untouched() {
        let host = crate::metrics::total_memory_mib();
        let (announced, said) = announced_memory_mib(host, None);
        assert_eq!(announced, host);
        assert_eq!(said, None);
        // And on this host, read for real: either nothing encloses the
        // runner — the figure is `MemTotal`, untouched — or something does,
        // and then the figure is the room and the line says so.
        let measured = measure();
        let (announced, said) = announced_memory_mib(host, measured.clone());
        match measured {
            None => {
                assert_eq!(announced, host, "nothing encloses this runner");
                assert_eq!(said, None);
            }
            Some(enclosure) => {
                let room = enclosure.room_mib();
                if host.is_some_and(|host| room >= host) {
                    assert_eq!(announced, host, "a ceiling above MemTotal bounds nothing");
                    assert_eq!(said, None);
                } else {
                    assert_eq!(announced, Some(room));
                    assert!(said
                        .expect("a lowered figure is said out loud")
                        .contains(&enclosure.cgroup));
                }
            }
        }
    }

    /// This host, answered with whatever it is. The figure, if there is one,
    /// has to be a plausible amount of memory — a bound read as a few bytes
    /// would be a runner that admits nothing, and that must fail here rather
    /// than on the server.
    #[test]
    fn this_host_answers_how_much_room_it_has() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let Some(found) = measure() else {
            // No enclosing limit: the host figure is the answer (D119), and
            // the test that this changes nothing is in `metrics`.
            return;
        };
        assert!(found.cgroup.starts_with('/'), "{found:?}");
        assert!(
            found.ceiling_bytes >= 1 << 20,
            "a ceiling of {} bytes is not a bound a runner could live under: {found:?}",
            found.ceiling_bytes
        );
        assert!(found.why().contains(&found.cgroup));
    }
}
