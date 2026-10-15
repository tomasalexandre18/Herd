//! Whether an `io.weight` written in a job's cgroup can reach the disk the
//! job actually works on (M16, D120).
//!
//! The cgroup core accepts `io.weight` on every machine that has the `io`
//! controller. The block layer is what honours it, and only two things in it
//! do: the **bfq** IO scheduler, and **blk-iocost**. So the question is about
//! a device, and the device that matters is the one under the job's working
//! directory — **not** the host's disks in general. D116 asked the host and
//! got the wrong answer here: no device on this project's server offers bfq,
//! so the report said inert, while blk-iocost runs with `ctrl=auto` on the
//! NVMe that carries the data root and weights its IO per cgroup.
//!
//! Two answers rather than one, because they are different facts:
//!
//! - **the data root is on ZFS.** No cgroup IO knob can ever reach it: ZFS
//!   does its pool IO from its own kernel threads and does not attribute it
//!   to the cgroup of the process that asked, so `io.weight` and `io.max`
//!   alike are inert there whatever the scheduler says. That is permanent and
//!   worth its own sentence — an operator told "inert" would go looking for a
//!   scheduler to change, and there is none that would help.
//! - **anything else** is a device, named, with the reason its weight does or
//!   does not bite.
//!
//! ## `ctrl=auto` enables itself, so the model is the question
//!
//! blk-iocost with `ctrl=auto` is **off until something writes a non-default
//! `io.weight`**, and that write is what turns it on — for any device whose
//! cost model is known. A check that only asked `enable=1` in `io.cost.qos`
//! would therefore read "no iocost" before Herd's first job and be wrong a
//! moment later, which is how D116 got it wrong. So a **known cost model** in
//! `io.cost.model` counts as "the weight will bite": the model is what the
//! controller needs in order to come up, and it is there whether the
//! controller has been woken yet or not.
//!
//! D23's rule is that a runner never claims a bound it does not have, so the
//! question is asked here and answered in the startup report either way.
//!
//! ## The other knob, and the difference that matters
//!
//! `io.weight` is a **share of contended bandwidth**: it decides who gets the
//! disk when two cgroups both want it, and it caps nothing — one job alone on
//! an idle machine saturates the disk at any weight. `io.max` is a **hard
//! ceiling** in bytes and IOPS per second, enforced by blk-throttle whatever
//! the IO scheduler is, which is why it is the lever for a host where one job
//! can saturate a disk. It is declared by an operator and absent by default
//! ([`crate::cgroup::IoCeiling`]).
//!
//! Neither bounds a job that writes an **absolute path outside its data
//! root**: that write lands on whatever device that path is on, and this
//! module's whole answer — and the device `io.max` names — is about the one
//! device the data root is on. Herd does not stop a job naming a path (D123,
//! D126), so that is a gap in both knobs and not a weakness of one.
//!
//! [`ceiling_device`] is the `io.max` half: which device a ceiling must name,
//! or why no ceiling can be written for a path at all. Note that ZFS is no
//! more reachable by `io.max` than by `io.weight` (D120).

use std::fs;
use std::path::{Path, PathBuf};

/// Where the kernel lists this process's mounts, with the device number of
/// each. Read rather than `stat`ed because a filesystem that is not one block
/// device (ZFS, btrfs over several) has a device number that maps to nothing,
/// and that difference is exactly what has to be reported.
const MOUNTINFO: &str = "/proc/self/mountinfo";

/// Where the block layer's cost model is switched on, per device.
const IO_COST_QOS: &str = "/sys/fs/cgroup/io.cost.qos";

/// Where the cost of a device's IO is described, per device. A device listed
/// here is one blk-iocost can come up for.
const IO_COST_MODEL: &str = "/sys/fs/cgroup/io.cost.model";

/// Where a device number becomes a directory.
const SYS_DEV_BLOCK: &str = "/sys/dev/block";

/// Whether the `io.weight` a job's cgroup carries reaches the disk that job
/// works on, and why.
///
/// There is always a `why`: "it bites" and "it is written and ignored" are
/// both facts an operator has to be able to read, and a question that could
/// not be answered is a sentence saying so rather than a silence — "we could
/// not tell" and "it works" must never look the same in a startup report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoWeightReach {
    /// Whether the kernel will weight this job's IO by the number written.
    pub bites: bool,
    pub why: String,
}

impl IoWeightReach {
    fn bites(why: String) -> Self {
        Self { bites: true, why }
    }

    fn inert(why: String) -> Self {
        Self { bites: false, why }
    }

    /// Why the weight cannot bite, where it cannot — `None` when it does.
    pub fn inert_reason(&self) -> Option<&str> {
        (!self.bites).then_some(self.why.as_str())
    }
}

/// Whether an `io.weight` written for a job under `data_root` reaches the
/// device that directory is on.
pub fn io_weight_reach(data_root: &Path) -> IoWeightReach {
    if !cfg!(target_os = "linux") {
        return IoWeightReach::inert(
            "this host is not Linux, so it has no cgroup v2 block-IO weighting".to_owned(),
        );
    }
    let mountinfo = match fs::read_to_string(MOUNTINFO) {
        Ok(text) => text,
        Err(error) => {
            return IoWeightReach::inert(format!(
                "{MOUNTINFO} cannot be read ({error}), so which device {} is on is unknown \
                 and nothing here can say whether an IO weight would reach it",
                data_root.display()
            ))
        }
    };
    let root = fs::canonicalize(data_root).unwrap_or_else(|_| data_root.to_path_buf());
    let Some(mount) = mount_holding(&mountinfo, &root) else {
        return IoWeightReach::inert(format!(
            "no mount in {MOUNTINFO} holds {}, so which device it is on is unknown",
            root.display()
        ));
    };
    // ZFS first, and by name: it has a device number that maps to no block
    // device, so the generic answer below would be true and useless. This one
    // says the thing an operator needs — that there is no knob to look for.
    if mount.kind == "zfs" {
        return IoWeightReach::inert(format!(
            "{} is on {} ({} from the ZFS pool {}), and ZFS does its pool IO from its own \
             kernel threads without attributing it to the cgroup that asked. No cgroup IO \
             knob reaches a ZFS dataset — not io.weight and not io.max, at any value and \
             under any scheduler — so this job's IO is bounded by nothing. Put work that \
             must be bounded on a block-device filesystem instead",
            root.display(),
            mount.point,
            mount.kind,
            mount.source
        ));
    }
    let Some(disk) = disk_of(&mount.device) else {
        return IoWeightReach::inert(format!(
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
        return IoWeightReach::bites(format!(
            "{} is on {} (device {}), whose IO scheduler is bfq, and bfq weights each \
             cgroup's IO by the io.weight it carries",
            root.display(),
            mount.point,
            disk.device
        ));
    }
    if let Some(how) = iocost_reaches(&disk.device) {
        return IoWeightReach::bites(format!(
            "{} is on {} (device {}), where blk-iocost {}, so the kernel weights this \
             job's IO by the number written",
            root.display(),
            mount.point,
            disk.device,
            how
        ));
    }
    IoWeightReach::inert(format!(
        "{} is on {} ({}, device {}), whose IO scheduler is {} and not bfq, and \
         blk-iocost knows no cost model for that device in {IO_COST_MODEL} and is not \
         enabled for it in {IO_COST_QOS}. Only those two honour a cgroup's io.weight, so \
         the weight is written and the kernel ignores it",
        root.display(),
        mount.point,
        mount.source,
        disk.device,
        scheduler.unwrap_or_else(|| "unreadable".to_owned()),
    ))
}

/// Kept for the callers that only care about the bad news.
pub fn io_weight_inert_reason(data_root: &Path) -> Option<String> {
    let reach = io_weight_reach(data_root);
    (!reach.bites).then_some(reach.why)
}

/// The device an `io.max` ceiling for jobs under `data_root` must name, and
/// where that device was found.
///
/// `io.max` is **per-device**: every line of it begins with a `major:minor`,
/// and a ceiling with no device is not a weaker ceiling but no ceiling at all.
/// So the first thing a ceiling needs is this answer, and the second thing it
/// needs is for the answer to be a refusal where there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CeilingDevice {
    /// `major:minor` of the **whole disk**, which is what `io.max` is keyed
    /// on: a partition has no request queue of its own, and the kernel
    /// accounts a partition's IO to its disk.
    pub device: String,
    /// The mount point the data root is on, for the startup report.
    pub mount_point: String,
    /// What that mount is made of, as `/proc/self/mountinfo` names it.
    pub source: String,
}

/// Which device a ceiling for `data_root` must name, or **why no ceiling can
/// be written for that path at all**.
///
/// The `Err` side is a startup refusal and not a warning, which is the one
/// place this parts company with [`io_weight_reach`]. A weight nobody honours
/// costs nothing — it is reported inert and the job runs — but a *ceiling*
/// nobody honours is a false promise: an operator who wrote `--job-io-max`
/// did it because one job can saturate this disk, and a runner that took the
/// option and bounded nothing would be the exact failure D116 and D129 are
/// about, on the one machine the whole project builds on.
pub fn ceiling_device(data_root: &Path) -> Result<CeilingDevice, String> {
    if !cfg!(target_os = "linux") {
        return Err(
            "this host is not Linux, so it has no cgroup v2 io.max to write a ceiling in"
                .to_owned(),
        );
    }
    let mountinfo = fs::read_to_string(MOUNTINFO).map_err(|error| {
        format!(
            "{MOUNTINFO} cannot be read ({error}), so which device {} is on is unknown — and \
             io.max is per-device, so there is no line to write",
            data_root.display()
        )
    })?;
    let root = fs::canonicalize(data_root).unwrap_or_else(|_| data_root.to_path_buf());
    let mount = ceiling_mount(&mountinfo, &root)?;
    let disk = disk_of(&mount.device).ok_or_else(|| {
        format!(
            "{} is on {} ({} from {}), which is not one block device with a request queue of \
             its own, so blk-throttle has no device to hold a ceiling against",
            root.display(),
            mount.point,
            mount.kind,
            mount.source
        )
    })?;
    Ok(CeilingDevice {
        device: disk.device,
        mount_point: mount.point,
        source: mount.source,
    })
}

/// The mount a ceiling for `root` would be written against, or the sentence
/// to refuse the startup with. Over the text of `mountinfo`, so the refusals
/// can be asserted on a host whose own disks are something else.
fn ceiling_mount(mountinfo: &str, root: &Path) -> Result<Mount, String> {
    let mount = mount_holding(mountinfo, root).ok_or_else(|| {
        format!(
            "no mount in {MOUNTINFO} holds {}, so which device it is on is unknown — and \
             io.max is per-device, so there is no line to write",
            root.display()
        )
    })?;
    if mount.kind == "zfs" {
        return Err(format!(
            "{} is on {} ({} from the ZFS pool {}), and ZFS does its pool IO from its own \
             kernel threads without attributing it to the cgroup that asked. No cgroup IO \
             knob reaches a ZFS dataset — not io.weight and not io.max, at any value — so a \
             ceiling written here would bound nothing. Put the data root on a block-device \
             filesystem, or leave the option out",
            root.display(),
            mount.point,
            mount.kind,
            mount.source
        ));
    }
    Ok(mount)
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

/// How blk-iocost reaches this disk, or `None` when it does not.
fn iocost_reaches(device: &str) -> Option<String> {
    let qos = fs::read_to_string(IO_COST_QOS).unwrap_or_default();
    let model = fs::read_to_string(IO_COST_MODEL).unwrap_or_default();
    iocost_reach(&qos, &model, device)
}

/// The decision, over the two files' contents, so it can be asserted without
/// a host that has blk-iocost.
///
/// `enable=1` is the controller already up. A **cost model** is the other
/// half: with `ctrl=auto` the controller comes up by itself the first time a
/// non-default `io.weight` is written, and Herd's own write is what does it
/// (D120) — so a device with a known model is one whose weight will bite,
/// even though nothing is enabled at the moment this is read.
///
/// The one case where a model is *not* enough: `enable=0 ctrl=user`, an
/// operator who set the parameters by hand and then switched the controller
/// off. The kernel keeps user parameters across a refresh, so Herd's write
/// does not bring it back up, and claiming a bound there would be exactly
/// the wrong half of D116's mistake made in the other direction.
fn iocost_reach(qos: &str, model: &str, device: &str) -> Option<String> {
    if let Some(line) = line_for(qos, device) {
        let mut words = line.split_whitespace();
        if words.any(|word| word == "enable=1") {
            return Some(format!(
                "is enabled for it ({}){}",
                line.trim(),
                no_latency_target(line)
            ));
        }
        if line.split_whitespace().any(|word| word == "ctrl=user") {
            return None;
        }
    }
    let line = line_for(model, device)?;
    // A model line with no read throughput in it describes nothing the
    // controller could cost IO with, so it is not a model.
    let rbps = line.split_whitespace().find_map(|word| {
        word.strip_prefix("rbps=")
            .and_then(|figure| figure.parse::<u64>().ok())
    })?;
    if rbps == 0 {
        return None;
    }
    Some(format!(
        "knows its cost model ({}) and runs with ctrl=auto, so the first non-default \
         io.weight written — this runner's own, on its first job — brings the controller \
         up for it",
        line.trim()
    ))
}

/// The sentence to add where blk-iocost is enabled with **no latency
/// target**, and nothing where it has one.
///
/// Measured on this project's server: `259:0 enable=1 ctrl=auto rpct=0.00
/// wpct=0.00 rlat=5000 wlat=5000`. `rpct` and `wpct` are the latency
/// percentiles iocost watches to decide the device is saturated, and at zero
/// it watches nothing: the controller **apportions** the device by weight and
/// never lowers the total load to protect latency.
///
/// That distinction is the whole reason `io.max` exists. A weight decides who
/// gets a saturated device; nothing in the weight path decides whether the
/// device gets saturated — so every weight on this host can be honoured
/// exactly while `/proc/pressure/io` reads `full avg60=43`, which is the
/// first consumer's freeze with the knob working as designed. An operator
/// reading "io.weight is enforced" has to be able to see that.
fn no_latency_target(qos_line: &str) -> &'static str {
    let percentile = |key: &str| {
        qos_line
            .split_whitespace()
            .find_map(|word| word.strip_prefix(key))
            .and_then(|figure| figure.parse::<f64>().ok())
    };
    match (percentile("rpct="), percentile("wpct=")) {
        (Some(read), Some(write)) if read == 0.0 && write == 0.0 => {
            ". Note rpct=0 and wpct=0: iocost watches no latency percentile here, so it \
             apportions this device by weight and never lowers the total load to protect \
             latency — every weight can be honoured while the device is saturated, which \
             is what io.max and nothing else bounds"
        }
        _ => "",
    }
}

fn line_for<'a>(text: &'a str, device: &str) -> Option<&'a str> {
    text.lines()
        .find(|line| line.split_whitespace().next() == Some(device))
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

    /// D120's correction, as an assertion. `enable=0` on a device with a
    /// known cost model and `ctrl=auto` is **not** a device whose weight is
    /// inert: the first non-default `io.weight` written turns the controller
    /// on, and Herd's own write is that write. Reading `enable=` alone is the
    /// mistake D116 made.
    #[test]
    fn a_cost_model_under_ctrl_auto_counts_even_before_anything_enabled_it() {
        let qos = "259:0 enable=0 ctrl=auto rpct=0.00 rlat=5000\n8:0 enable=1 ctrl=user\n";
        let model = "259:0 ctrl=auto model=linear rbps=3102524156 rseqiops=724816\n";
        // The NVMe: nothing is enabled yet, and the weight will still bite.
        let how = iocost_reach(qos, model, "259:0").expect("a known model reaches the device");
        assert!(how.contains("ctrl=auto"), "{how}");
        assert!(how.contains("first non-default"), "{how}");
        // Already enabled, which is the simple case and says so differently.
        let how = iocost_reach(qos, model, "8:0").expect("enable=1 is the controller up");
        assert!(how.contains("is enabled for it"), "{how}");
        // And a controller enabled with no latency percentile to watch says
        // so, because a weight that is perfectly honoured on a saturated
        // device is the first consumer's freeze. Measured on this host:
        // `enable=1 ctrl=auto rpct=0.00 wpct=0.00`.
        let how = iocost_reach(
            "259:0 enable=1 ctrl=auto rpct=0.00 rlat=5000 wpct=0.00 wlat=5000 min=1.00\n",
            "",
            "259:0",
        )
        .expect("the controller is up");
        assert!(how.contains("rpct=0 and wpct=0"), "{how}");
        assert!(
            how.contains("never lowers the total load"),
            "the line has to say what the weight cannot do, or it reads as a bound \
             against saturation: {how}"
        );
        // A controller watching a real percentile says nothing extra: it is
        // doing what a reader would assume it does.
        let how = iocost_reach(
            "7:0 enable=1 ctrl=user rpct=95.00 rlat=5000 wpct=95.00 wlat=5000\n",
            "",
            "7:0",
        )
        .expect("the controller is up");
        assert!(!how.contains("rpct=0"), "{how}");
        // A device with neither: no reach.
        assert_eq!(iocost_reach(qos, model, "253:0"), None);
        assert_eq!(iocost_reach("", "", "8:0"), None);
        // A model line that describes no throughput is not a model.
        assert_eq!(
            iocost_reach("", "7:0 ctrl=auto model=linear rbps=0 wbps=0\n", "7:0"),
            None
        );
        // And an operator who set the parameters by hand and switched the
        // controller off is not somebody whose weight comes back on its own:
        // the kernel keeps user parameters, so Herd's write changes nothing
        // and claiming a bound there would be a lie in the other direction.
        assert_eq!(
            iocost_reach(
                "9:0 enable=0 ctrl=user rpct=95.00 rlat=5000\n",
                "9:0 ctrl=user model=linear rbps=1000000 wbps=1000000\n",
                "9:0"
            ),
            None
        );
    }

    /// `io.max` is per-device, and the device it must name is the **disk**
    /// the data root is on, never the partition: a partition has no request
    /// queue of its own and the kernel accounts its IO to the disk. So a
    /// ceiling for a path on `nvme0n1p1` is written against `nvme0n1`'s
    /// number, and a ceiling that named the partition would be a line the
    /// kernel keeps and never applies.
    #[test]
    fn a_ceiling_names_the_disk_its_path_is_on_and_not_the_partition() {
        if !cfg!(target_os = "linux") {
            return;
        }
        // The whole answer for this host, whatever it is: a device or a
        // refusal naming the path, never a silence and never a panic.
        let here = std::env::current_dir().expect("this test has a working directory");
        match ceiling_device(&here) {
            Ok(device) => {
                // The device number is a device number, and it is the one
                // `/sys/dev/block` agrees is a disk with a queue.
                let (major, minor) = device
                    .device
                    .split_once(':')
                    .expect("a device is major:minor");
                assert!(major.parse::<u32>().is_ok(), "{device:?}");
                assert!(minor.parse::<u32>().is_ok(), "{device:?}");
                assert!(
                    Path::new(SYS_DEV_BLOCK)
                        .join(&device.device)
                        .join("queue")
                        .is_dir(),
                    "a ceiling must name a device with a request queue of its own, and \
                     {} has none — io.max would keep the line and apply nothing: {device:?}",
                    device.device
                );
                assert!(here.starts_with(&device.mount_point), "{device:?}");
            }
            Err(why) => {
                assert!(
                    why.contains(&here.display().to_string()) || why.contains("mountinfo"),
                    "a refusal names the path it is about: {why}"
                );
            }
        }
    }

    /// The refusal, which is the half that matters: a ceiling asked for on a
    /// path that cannot carry one must be a startup refusal naming the path
    /// and the reason, not a warning and a weaker bound. ZFS is the case this
    /// project actually has (D120, D126) — `/` and `/tmp` here — and no
    /// cgroup IO knob reaches it at any value.
    #[test]
    fn a_path_no_ceiling_can_reach_is_a_refusal_naming_it_and_why() {
        let mountinfo = "\
25 1 0:24 / / rw,relatime shared:1 - zfs rpool/root rw,xattr
48 25 259:1 / /mnt/ssd_980 rw,relatime shared:2 - ext4 /dev/nvme0n1p1 rw
";
        // `/var/lib/herd-runner` is the path D126 moved the deployment off,
        // and it is ZFS. A ceiling asked for there is a refusal, and the
        // refusal has to carry four things: the path, the filesystem, that
        // **neither** knob reaches it at any value, and what to do instead.
        let why = ceiling_mount(mountinfo, Path::new("/var/lib/herd-runner"))
            .expect_err("no cgroup IO knob reaches a ZFS dataset");
        assert!(why.contains("/var/lib/herd-runner"), "{why}");
        assert!(why.contains("zfs") && why.contains("rpool/root"), "{why}");
        assert!(
            why.contains("not io.weight and not io.max"),
            "the refusal must say that no value would help, or the operator goes looking \
             for a knob to turn: {why}"
        );
        assert!(why.contains("block-device filesystem"), "{why}");
        // The same path on the ext4 NVMe is a device, not a refusal: whether
        // a ceiling can be written depends on where the job works and not on
        // the host (D120).
        let mount = ceiling_mount(mountinfo, Path::new("/mnt/ssd_980/herd-runner"))
            .expect("an ext4 mount can carry a ceiling");
        assert_eq!(mount.device, "259:1");
        // A mount nothing holds, and a path on a filesystem that is no block
        // device, are each a sentence naming the path.
        let why = ceiling_mount("", Path::new("/anywhere")).expect_err("no mount holds it at all");
        assert!(
            why.contains("/anywhere") && why.contains("per-device"),
            "{why}"
        );
        if cfg!(target_os = "linux") {
            let why = ceiling_device(Path::new("/proc/self"))
                .expect_err("/proc is no block device, so no ceiling can be written for it");
            assert!(why.contains("/proc"), "{why}");
        }
    }

    /// Whatever this host turns out to be, the question is answered with a
    /// sentence either way — never a panic, and never a silence that reads
    /// the same as "it works".
    #[test]
    fn this_host_answers_whether_its_io_weight_bites() {
        let reach = io_weight_reach(Path::new("."));
        assert!(
            reach.why.len() > 40 && reach.why.contains('/'),
            "the answer must be a sentence naming what was looked at: {reach:?}"
        );
        assert_eq!(reach.inert_reason().is_none(), reach.bites);
    }
}
