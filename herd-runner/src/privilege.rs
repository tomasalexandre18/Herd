//! A runner is unprivileged, and stays unprivileged (E-46).
//!
//! Three rules, in order of how much they matter:
//!
//! 1. **A runner refuses to run as root.** Not a warning: a refusal at
//!    startup, before it joins a hub, because a root runner executing a
//!    consumer's command is the one failure no later cap can undo.
//! 2. **A runner never invokes `sudo`.** There is no code path that does, and
//!    a test walks this crate's own sources to keep it that way.
//! 3. **Anything that genuinely needs privilege goes through a configured
//!    fixed list.** The owner of the machine writes down, by absolute path,
//!    the helper commands a runner may call; a runner may call those and
//!    nothing else. The list is the whole mechanism — never "a command the
//!    hub sends", which would be a privileged plugin system (invariant 1).
//!
//! M4 needs no helper, so none is invoked here. What exists is the refusal
//! and the shape of the list, so that the first thing that needs privilege
//! finds a gate rather than inventing one.
//!
//! ## What "unprivileged" does not cover
//!
//! A uid is not the whole of E-46. A user in the `docker` group is
//! root-equivalent through the daemon's socket; a user in `wheel` can become
//! root with one password. The runner cannot refuse those without refusing to
//! run on the machines Herd exists for — the owner's own server is one — so it
//! **names them at startup as a warning** and lets the machine's owner decide.
//! Saying nothing would be the failure: it would let a deployment read as
//! unprivileged when it is not.

/// The effective user id, or `None` where there is no such notion.
#[cfg(unix)]
fn effective_uid() -> Option<u32> {
    // Safe: geteuid reads one integer out of the kernel and cannot fail.
    Some(unsafe { libc::geteuid() })
}

#[cfg(not(unix))]
fn effective_uid() -> Option<u32> {
    None
}

/// The real user id, which must be checked too: a binary that is setuid-root
/// runs with uid 0 and a non-zero euid is not what keeps it harmless.
#[cfg(unix)]
fn real_uid() -> Option<u32> {
    // Safe for the same reason as geteuid.
    Some(unsafe { libc::getuid() })
}

#[cfg(not(unix))]
fn real_uid() -> Option<u32> {
    None
}

/// Refuse to go on if this process has root's identity in any form.
pub fn refuse_if_privileged() -> Result<(), String> {
    match (real_uid(), effective_uid()) {
        (Some(0), _) | (_, Some(0)) => Err(
            "this runner is running as root and refuses to start: a job must never \
             run with more rights than the machine's owner has (E-46). Start it as \
             an unprivileged user."
                .to_owned(),
        ),
        _ => Ok(()),
    }
}

/// Whether a file carries the set-user-id or set-group-id bit, asked of the
/// file itself. This is the real check behind the names in
/// [`ESCALATION_TOOLS`]: a setuid binary runs as its owner whatever it is
/// called, and a tool named after nothing on that list is just as dangerous
/// if it is setuid-root.
#[cfg(unix)]
fn set_user_or_group_id(path: &std::path::Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    // Resolved, not the symlink: a link to sudo is sudo.
    let metadata = std::fs::metadata(path)?;
    Ok(metadata.mode() & (libc::S_ISUID | libc::S_ISGID) != 0)
}

#[cfg(not(unix))]
fn set_user_or_group_id(path: &std::path::Path) -> std::io::Result<bool> {
    std::fs::metadata(path)?;
    Ok(false)
}

/// Whether this user may write a path, asked of the kernel rather than
/// guessed from the mode bits. Lives here because "may this user" is a
/// privilege question; `cgroup` uses it for the one file cgroup v2 needs
/// writable before a job's process can be moved.
#[cfg(unix)]
pub(crate) fn writable_by_this_user(path: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // Safe: access(2) reads one path and returns an integer.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(unix))]
pub(crate) fn writable_by_this_user(_path: &std::path::Path) -> bool {
    false
}

/// Group memberships that make a user root in all but name.
///
/// `docker` and the container runtimes are the clearest: the daemon runs as
/// root and will mount the host's filesystem wherever its client asks.
/// `wheel`, `sudo` and `admin` are one password away from root.
#[cfg(unix)]
const ROOT_EQUIVALENT_GROUPS: &[&str] = &[
    "docker", "podman", "lxd", "incus", "libvirt", "libvirtd", "wheel", "sudo", "admin",
];

/// Sockets that hand out root to whoever can write to them.
#[cfg(unix)]
const RUNTIME_SOCKETS: &[&str] = &[
    "/run/docker.sock",
    "/var/run/docker.sock",
    "/run/podman/podman.sock",
    "/run/containerd/containerd.sock",
    "/run/crio/crio.sock",
    "/var/run/libvirt/libvirt-sock",
];

/// Everything about this deployment that makes E-46's "unprivileged" less
/// than true, each as one sentence naming the specific reason.
///
/// A warning, never a refusal: the first machine Herd runs on is the owner's
/// own server, whose user is in `docker` because that server runs the hub in
/// docker compose. Refusing there would make Herd unusable on exactly the
/// hosts it was built for, and that trade is the machine owner's to make —
/// not ours. What is not his to lose is knowing.
#[cfg(unix)]
pub fn root_equivalent_warnings() -> Vec<String> {
    let mut warnings = Vec::new();
    for group in root_equivalent_memberships() {
        warnings.push(format!(
            "this runner's user is in the {group:?} group, which is root-equivalent on \
             most hosts, so a job could leave every cap behind by asking something \
             else to run as root. E-46's \"unprivileged\" is not true of this \
             deployment. Run the runner as a user that is in no such group if that \
             matters to you."
        ));
    }
    // Canonical, so that /var/run/docker.sock and /run/docker.sock — the same
    // socket through a symlink on most hosts — are one warning and not two.
    let mut seen = Vec::new();
    for socket in RUNTIME_SOCKETS {
        let path = std::path::Path::new(socket);
        if !path.exists() || !writable_by_this_user(path) {
            continue;
        }
        let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        warnings.push(format!(
            "this runner's user can write {socket}, which is a container or VM \
             runtime and therefore root on this host: a job could start a \
             privileged container and escape every cap. E-46's \"unprivileged\" is \
             not true of this deployment."
        ));
    }
    warnings
}

#[cfg(not(unix))]
pub fn root_equivalent_warnings() -> Vec<String> {
    Vec::new()
}

/// The names of this process's groups that are root-equivalent.
#[cfg(unix)]
fn root_equivalent_memberships() -> Vec<String> {
    let mut named = Vec::new();
    let mine = own_group_ids();
    let Ok(text) = std::fs::read_to_string("/etc/group") else {
        return named;
    };
    for line in text.lines() {
        // name:password:gid:members
        let mut fields = line.split(':');
        let Some(name) = fields.next() else { continue };
        if !ROOT_EQUIVALENT_GROUPS.contains(&name) {
            continue;
        }
        let gid: Option<u32> = fields.nth(1).and_then(|gid| gid.parse().ok());
        if gid.is_some_and(|gid| mine.contains(&gid)) {
            named.push(name.to_owned());
        }
    }
    named
}

/// This process's real, effective and supplementary group ids.
#[cfg(unix)]
fn own_group_ids() -> Vec<u32> {
    // Safe: getgid and getegid read one integer each.
    let mut ids = unsafe { vec![libc::getgid(), libc::getegid()] };
    // Safe: getgroups with a null pointer and 0 only asks for the count, then
    // the buffer is sized to that count before it is filled.
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if count > 0 {
        let mut buffer = vec![0 as libc::gid_t; count as usize];
        // Safe: the buffer holds `count` gid_t, which is what is announced.
        let filled = unsafe { libc::getgroups(count, buffer.as_mut_ptr()) };
        if filled > 0 {
            buffer.truncate(filled as usize);
            ids.extend(buffer);
        }
    }
    ids
}

/// The fixed list of helper commands this runner may call with privilege.
///
/// Empty on every machine until something actually needs one. A list, never
/// a pattern and never a command from the wire: the only answer to "may I run
/// this?" is whether the machine's owner wrote that exact path down.
#[derive(Debug, Clone, Default)]
pub struct PrivilegedHelpers {
    allowed: Vec<String>,
}

/// The tools whose whole purpose is to run something as somebody else. A
/// fixed list that contains one of these is not a fixed list: it allows
/// everything, through one entry that looks like one command.
///
/// Matched on the file's own name, never as a substring of the path — a
/// directory called `/opt/sudoers-docs/bin/mytool` is not an escalation tool,
/// and the earlier substring check refused it while accepting `/usr/bin/doas`.
const ESCALATION_TOOLS: &[&str] = &[
    "sudo",
    "sudoedit",
    "doas",
    "pkexec",
    "run0",
    "su",
    "runuser",
    "setpriv",
    "machinectl",
    "systemd-run",
    "nsenter",
    "unshare",
    "chroot",
    "capsh",
];

impl PrivilegedHelpers {
    /// Build the list, refusing anything that is not one named, ordinary,
    /// unprivileged file that the machine's owner wrote down.
    ///
    /// Four refusals, each about a different way a "fixed list" stops being
    /// one:
    ///
    /// 1. a relative path, which `PATH` would resolve — so the list would not
    ///    decide what runs;
    /// 2. a general-purpose escalation tool, which turns one entry into
    ///    permission to run anything;
    /// 3. a file that does not exist, because then nothing about it can be
    ///    checked and the entry is a promise rather than a path;
    /// 4. a **setuid or setgid** file, checked on the file itself and not on
    ///    its name: that is what an escalation tool actually is, and it
    ///    catches the ones this list has never heard of.
    pub fn new(entries: Vec<String>) -> Result<Self, String> {
        let mut allowed = Vec::with_capacity(entries.len());
        for entry in entries {
            let trimmed = entry.trim().to_owned();
            if trimmed.is_empty() {
                return Err("a privileged helper cannot be an empty path".to_owned());
            }
            if !trimmed.starts_with('/') {
                return Err(format!(
                    "the privileged helper {trimmed:?} is not an absolute path, and a \
                     helper found through PATH is not a fixed list"
                ));
            }
            let path = std::path::Path::new(&trimmed);
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if ESCALATION_TOOLS.contains(&name.as_str()) {
                return Err(format!(
                    "the privileged helper {trimmed:?} is {name}, a general-purpose \
                     escalation tool: a list containing one allows everything, and a \
                     runner escalates through the fixed list itself (E-46)"
                ));
            }
            match set_user_or_group_id(path) {
                Ok(false) => {}
                Ok(true) => {
                    return Err(format!(
                        "the privileged helper {trimmed:?} is setuid or setgid, so it \
                         runs as somebody else by itself: that is an escalation tool \
                         whatever it is called (E-46)"
                    ))
                }
                Err(error) => {
                    return Err(format!(
                        "the privileged helper {trimmed:?} cannot be checked: {error}. A \
                         helper this runner cannot look at is not a helper it may call."
                    ))
                }
            }
            if !allowed.contains(&trimmed) {
                allowed.push(trimmed);
            }
        }
        Ok(Self { allowed })
    }

    /// The list as configured, in the order it was given.
    pub fn allowed(&self) -> &[String] {
        &self.allowed
    }

    /// The helper to run for this request, or a refusal naming it.
    ///
    /// Nothing in M4 calls this. It exists so that the first caller that
    /// needs privilege has one gate to go through and one refusal to read.
    pub fn resolve(&self, command: &str) -> Result<&str, String> {
        self.allowed
            .iter()
            .find(|allowed| allowed.as_str() == command)
            .map(String::as_str)
            .ok_or_else(|| {
                if self.allowed.is_empty() {
                    format!(
                        "{command:?} needs privilege and this runner has no privileged \
                         helper configured"
                    )
                } else {
                    format!(
                        "{command:?} is not one of this runner's {} configured privileged \
                         helpers",
                        self.allowed.len()
                    )
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The suite itself must not be running as root, or it would be asserting
    /// nothing about the refusal below.
    #[test]
    fn an_unprivileged_process_may_start() {
        assert_eq!(
            refuse_if_privileged(),
            Ok(()),
            "this test suite is running as root, so it cannot test the refusal"
        );
    }

    #[test]
    fn a_helper_must_be_an_absolute_path() {
        let error = PrivilegedHelpers::new(vec!["mount".to_owned()]).expect_err("relative");
        assert!(error.contains("mount") && error.contains("PATH"), "{error}");
        let error = PrivilegedHelpers::new(vec!["  ".to_owned()]).expect_err("empty");
        assert!(error.contains("empty"), "{error}");
    }

    /// Rule 2 is a rule about the list too: a list containing an escalation
    /// tool is a list that allows everything.
    ///
    /// The old check was `path.contains("sudo")`, which accepted
    /// `/usr/bin/doas` — and `pkexec`, and `run0`, and `setpriv`. Every one of
    /// them is refused by its own name now, whether or not this host has it.
    #[test]
    fn no_escalation_tool_may_be_a_helper() {
        for tool in ESCALATION_TOOLS {
            for directory in ["/run/wrappers/bin", "/usr/bin", "/nix/store/abc-x/bin"] {
                let path = format!("{directory}/{tool}");
                let error = PrivilegedHelpers::new(vec![path.clone()])
                    .expect_err("an escalation tool is not a fixed list");
                assert!(
                    error.contains(tool) && error.contains("escalation tool"),
                    "the refusal must name the tool: {error}"
                );
            }
        }
        // A path that merely contains the letters is an ordinary path: the
        // old substring check refused this one and allowed /usr/bin/doas.
        let temp = crate::testing::TempDir::new("not-an-escalator");
        let innocent = temp.path().join("sudoers-lint");
        std::fs::write(&innocent, b"#!/bin/sh\n").expect("the fixture writes");
        PrivilegedHelpers::new(vec![innocent.display().to_string()])
            .expect("a tool whose name merely contains those letters is allowed");
    }

    /// A file that is setuid or setgid *is* an escalation tool, whatever it is
    /// called, and that is checked on the file rather than on its name.
    #[test]
    #[cfg(unix)]
    fn a_setuid_helper_is_refused_whatever_it_is_called() {
        use std::os::unix::fs::PermissionsExt;

        let temp = crate::testing::TempDir::new("setuid-helper");
        let innocent = temp.path().join("my-very-own-helper");
        std::fs::write(&innocent, b"#!/bin/sh\n").expect("the fixture writes");
        std::fs::set_permissions(&innocent, std::fs::Permissions::from_mode(0o755))
            .expect("chmod 755");
        PrivilegedHelpers::new(vec![innocent.display().to_string()])
            .expect("an ordinary file this user owns is a usable helper");

        std::fs::set_permissions(&innocent, std::fs::Permissions::from_mode(0o4755))
            .expect("chmod 4755");
        let error = PrivilegedHelpers::new(vec![innocent.display().to_string()])
            .expect_err("a setuid file is an escalation tool");
        assert!(error.contains("setuid or setgid"), "{error}");

        std::fs::set_permissions(&innocent, std::fs::Permissions::from_mode(0o2755))
            .expect("chmod 2755");
        let error = PrivilegedHelpers::new(vec![innocent.display().to_string()])
            .expect_err("a setgid file is an escalation tool");
        assert!(error.contains("setuid or setgid"), "{error}");
    }

    /// A helper that is not there cannot be checked, so it is not a helper.
    #[test]
    fn a_helper_that_does_not_exist_is_refused_naming_it() {
        let error = PrivilegedHelpers::new(vec!["/nowhere/at/all/helper".to_owned()])
            .expect_err("a path that is not there cannot be checked");
        assert!(error.contains("/nowhere/at/all/helper"), "{error}");
        assert!(error.contains("cannot be checked"), "{error}");
    }

    #[test]
    fn only_a_listed_helper_resolves_and_a_refusal_names_the_command() {
        let temp = crate::testing::TempDir::new("helper-list");
        let helper = temp.path().join("fixture-helper");
        std::fs::write(&helper, b"#!/bin/sh\n").expect("the fixture writes");
        let path = helper.display().to_string();
        let helpers =
            PrivilegedHelpers::new(vec![path.clone(), path.clone()]).expect("a valid list");
        assert_eq!(helpers.allowed().len(), 1, "a repeated entry is one entry");
        assert_eq!(helpers.resolve(&path), Ok(path.as_str()));
        let error = helpers
            .resolve("/bin/rm")
            .expect_err("an unlisted command is refused");
        assert!(error.contains("/bin/rm"), "{error}");
        assert!(
            error.contains('1'),
            "the refusal names the list's size: {error}"
        );
    }

    #[test]
    fn with_no_helpers_configured_every_privileged_request_is_refused_by_name() {
        let helpers = PrivilegedHelpers::default();
        assert!(helpers.allowed().is_empty());
        let error = helpers
            .resolve("/sbin/modprobe")
            .expect_err("nothing is allowed");
        assert!(error.contains("/sbin/modprobe"), "{error}");
        assert!(
            error.contains("no privileged helper"),
            "the refusal must say the list is empty: {error}"
        );
    }

    /// Rule 2, enforced against the code rather than against a promise.
    ///
    /// This file is the one place in the runner allowed to name an escalation
    /// tool, and it names it only to refuse it. Every other module must be
    /// unable to reach one, so the test reads the crate's own sources: if a
    /// later milestone needs privilege, it adds a path to the fixed list, and
    /// this test is what stops it from reaching for a general-purpose tool
    /// instead. Comments are exempt; code is not.
    #[test]
    fn no_module_but_this_one_can_reach_an_escalation_tool() {
        let escalators = ["sudo", "doas", "pkexec", "runuser", "setpriv"];
        let sources = crate::testing::source_files();
        assert!(
            sources.len() > 5,
            "the walk found only {} source files, so it is not checking the crate",
            sources.len()
        );
        let mut checked = 0usize;
        for (path, text) in sources {
            if path.file_name().is_some_and(|name| name == "privilege.rs") {
                continue;
            }
            checked += 1;
            for (number, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                for escalator in escalators {
                    assert!(
                        !code.contains(escalator),
                        "{}:{} names {escalator}, which only privilege.rs may do: {line}",
                        path.display(),
                        number + 1
                    );
                }
            }
        }
        assert!(checked > 4, "only {checked} modules were checked");
    }
}
