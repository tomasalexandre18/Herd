//! Whether an `io.weight` written in a job's cgroup can do anything at all
//! on this host (M16).
//!
//! The cgroup core accepts `io.weight` on every machine that has the `io`
//! controller. The block layer is what honours it, and only two things in it
//! do: the **bfq** IO scheduler, and **blk-iocost** — the `io.cost.qos`
//! model, which is off until an operator turns it on. On a multiqueue NVMe
//! whose scheduler is `none`, which is the default on this project's own
//! server, neither is in play: the write succeeds, the file reads back the
//! number, and no IO is ever weighted by it.
//!
//! D23's rule is that a runner never claims a bound it does not have, so the
//! question is asked here and answered in the startup report. The runner
//! still writes the weight: it costs nothing, and the day an operator sets a
//! device to bfq or enables `io.cost` it starts to bite without a redeploy.
//!
//! What is *not* done here: an absolute `io.max` (bytes and IOPS per device),
//! which is the only IO lever that works whatever the scheduler is. It is a
//! hard per-device ceiling, so it belongs to the same family as `cpu.max` —
//! something an operator declares — and nobody has asked for it yet.

use std::fs;
use std::path::{Path, PathBuf};

/// Where the kernel lists this process's mounts, with the device number of
/// each. Read rather than `stat`ed because a filesystem that is not one block
/// device (ZFS, btrfs over several) has a device number that maps to nothing,
/// and that difference is exactly what has to be reported.
const MOUNTINFO: &str = "/proc/self/mountinfo";

/// Where the block layer's cost model is configured, per device.
const IO_COST_QOS: &str = "/sys/fs/cgroup/io.cost.qos";

/// Where a device number becomes a directory.
const SYS_DEV_BLOCK: &str = "/sys/dev/block";

/// Why an `io.weight` written for a job under `data_root` cannot bite, or
/// `None` when this host really does weight IO.
///
/// Never `None` out of ignorance: a question that cannot be answered is a
/// sentence saying so, because "we could not tell" and "it works" must not
/// look the same in a startup report.
pub fn io_weight_inert_reason(data_root: &Path) -> Option<String> {
    if !cfg!(target_os = "linux") {
        return Some(
            "this host is not Linux, so it has no cgroup v2 block-IO weighting".to_owned(),
        );
    }
    let mountinfo = match fs::read_to_string(MOUNTINFO) {
        Ok(text) => text,
        Err(error) => {
            return Some(format!(
                "{MOUNTINFO} cannot be read ({error}), so which device {} is on is unknown \
                 and nothing here can say whether an IO weight would reach it",
                data_root.display()
            ))
        }
    };
    let root = fs::canonicalize(data_root).unwrap_or_else(|_| data_root.to_path_buf());
    let Some(mount) = mount_holding(&mountinfo, &root) else {
        return Some(format!(
            "no mount in {MOUNTINFO} holds {}, so which device it is on is unknown",
            root.display()
        ));
    };
    let Some(disk) = disk_of(&mount.device) else {
        return Some(format!(
            "{} is on {} ({} from {}), which is not one block device with a request queue \
             of its own, so the block layer has no per-cgroup weighting to apply to it",
            root.display(),
            mount.point,
            mount.kind,
            mount.source
        ));
    };
    let scheduler = active_scheduler(&disk.queue);
    if scheduler.as_deref() == Some("bfq") {
        return None;
    }
    if iocost_is_enabled(&disk.device) {
        return None;
    }
    Some(format!(
        "{} is on {} ({}, device {}), whose IO scheduler is {} and not bfq, and \
         blk-iocost is not enabled for that device in {IO_COST_QOS}. Only those two \
         honour a cgroup's io.weight, so the weight is written and the kernel ignores it",
        root.display(),
        mount.point,
        mount.source,
        disk.device,
        scheduler.unwrap_or_else(|| "unreadable".to_owned()),
    ))
}

/// One line of `/proc/self/mountinfo`, as much of it as this is about.
#[derive(Debug, PartialEq)]
struct Mount {
    /// `major:minor`, the kernel's own device number for the filesystem.
    device: String,
    point: String,
    kind: String,
    source: String,
}

/// The mount a path is on: the longest mount point that holds it.
///
/// Longest wins because mounts nest — `/` holds everything and `/mnt/ssd_980`
/// holds the data root — and the device that matters is the innermost one.
fn mount_holding(mountinfo: &str, path: &Path) -> Option<Mount> {
    let mut best: Option<(usize, Mount)> = None;
    for line in mountinfo.lines() {
        // `36 35 98:0 /from /at rw,… - ext4 /dev/sda1 rw` — the fields before
        // the lone "-" are fixed in number only up to the optional ones, so
        // the filesystem type and source are taken from after the separator.
        let mut fields = line.split_whitespace();
        let (Some(_id), Some(_parent), Some(device), Some(_root), Some(point)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        let after = line.split(" - ").nth(1).unwrap_or_default();
        let mut tail = after.split_whitespace();
        let kind = tail.next().unwrap_or("an unnamed filesystem");
        let source = tail.next().unwrap_or("an unnamed source");
        // Mount points are written with octal escapes for spaces and tabs;
        // a path holding one is not worth a parser here, and a mount point
        // that does not match simply loses to the one that does.
        let candidate = Path::new(point);
        if !path.starts_with(candidate) {
            continue;
        }
        let depth = candidate.components().count();
        if best.as_ref().is_some_and(|(deepest, _)| *deepest >= depth) {
            continue;
        }
        best = Some((
            depth,
            Mount {
                device: device.to_owned(),
                point: point.to_owned(),
                kind: kind.to_owned(),
                source: source.to_owned(),
            },
        ));
    }
    best.map(|(_, mount)| mount)
}

/// The whole disk behind a device number, which is where the request queue
/// and the IO scheduler live: a partition has neither of its own.
#[derive(Debug)]
struct Disk {
    /// `major:minor` of the disk, which is what `io.cost.qos` is keyed on.
    device: String,
    queue: PathBuf,
}

fn disk_of(device: &str) -> Option<Disk> {
    let entry = Path::new(SYS_DEV_BLOCK).join(device);
    let own = entry.join("queue");
    if own.is_dir() {
        return Some(Disk {
            device: device.to_owned(),
            queue: own,
        });
    }
    // A partition: its parent directory is the disk, and that disk's own
    // device number is what the cost model names.
    let parent = entry.join("..");
    let queue = parent.join("queue");
    if !queue.is_dir() {
        return None;
    }
    let number = fs::read_to_string(parent.join("dev")).ok()?;
    Some(Disk {
        device: number.trim().to_owned(),
        queue,
    })
}

/// The scheduler in brackets in `queue/scheduler`, which is the active one.
fn active_scheduler(queue: &Path) -> Option<String> {
    let text = fs::read_to_string(queue.join("scheduler")).ok()?;
    Some(chosen_scheduler(&text))
}

/// `none [mq-deadline] kyber` is mq-deadline; a file with no brackets names
/// the only scheduler there is.
fn chosen_scheduler(text: &str) -> String {
    for word in text.split_whitespace() {
        if let Some(chosen) = word.strip_prefix('[').and_then(|w| w.strip_suffix(']')) {
            return chosen.to_owned();
        }
    }
    text.trim().to_owned()
}

/// Whether blk-iocost is turned on for this disk, which is the other way a
/// cgroup's `io.weight` becomes real.
fn iocost_is_enabled(device: &str) -> bool {
    let Ok(text) = fs::read_to_string(IO_COST_QOS) else {
        return false;
    };
    iocost_enables(&text, device)
}

fn iocost_enables(qos: &str, device: &str) -> bool {
    qos.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some(device) && words.any(|word| word == "enable=1")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mounts nest, and the device that governs a path's IO is the innermost
    /// one. A prefix match on strings would also read `/mnt/ssd` as holding
    /// `/mnt/ssd_980`, which is a different disk.
    #[test]
    fn the_innermost_mount_wins_and_a_name_prefix_is_not_a_mount() {
        let mountinfo = "\
25 1 0:24 / / rw,relatime shared:1 - zfs rpool/root rw,xattr
48 25 259:1 / /mnt/ssd_980 rw,relatime shared:2 - ext4 /dev/nvme0n1p1 rw
52 25 8:1 / /mnt/ssd rw,relatime shared:3 - ext4 /dev/sda1 rw
";
        let mount = mount_holding(mountinfo, Path::new("/mnt/ssd_980/herd/data"))
            .expect("some mount holds every path");
        assert_eq!(mount.device, "259:1");
        assert_eq!(mount.point, "/mnt/ssd_980");
        assert_eq!(mount.source, "/dev/nvme0n1p1");

        let elsewhere =
            mount_holding(mountinfo, Path::new("/home/someone")).expect("the root holds it");
        assert_eq!(elsewhere.device, "0:24");
        assert_eq!(elsewhere.kind, "zfs");
    }

    #[test]
    fn the_active_scheduler_is_the_one_in_brackets() {
        assert_eq!(chosen_scheduler("[none] mq-deadline kyber\n"), "none");
        assert_eq!(
            chosen_scheduler("none [mq-deadline] kyber\n"),
            "mq-deadline"
        );
        assert_eq!(chosen_scheduler("none mq-deadline [bfq]\n"), "bfq");
        assert_eq!(chosen_scheduler("bfq\n"), "bfq");
    }

    /// An `io.cost.qos` line exists for a device long before it is turned on,
    /// so the question is `enable=1` and never "is the device listed".
    #[test]
    fn a_cost_model_counts_only_when_it_is_enabled() {
        let qos = "259:0 enable=0 ctrl=auto rpct=95.00 rlat=5000\n8:0 enable=1 ctrl=user\n";
        assert!(!iocost_enables(qos, "259:0"));
        assert!(iocost_enables(qos, "8:0"));
        assert!(!iocost_enables(qos, "253:0"));
        assert!(!iocost_enables("", "8:0"));
    }

    /// Whatever this host turns out to be, the question is answered with
    /// either `None` or a sentence — never a panic, and never a `None` that
    /// means "could not tell".
    #[test]
    fn this_host_answers_whether_its_io_weight_bites() {
        match io_weight_inert_reason(Path::new(".")) {
            None => {
                // bfq or blk-iocost. Nothing more to assert: the next
                // assertion would be about this machine's disks.
            }
            Some(why) => assert!(
                why.len() > 40 && why.contains('/'),
                "an inert reason must be a sentence naming what was looked at: {why}"
            ),
        }
    }
}
