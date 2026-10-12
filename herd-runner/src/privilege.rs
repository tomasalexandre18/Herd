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

/// The fixed list of helper commands this runner may call with privilege.
///
/// Empty on every machine until something actually needs one. A list, never
/// a pattern and never a command from the wire: the only answer to "may I run
/// this?" is whether the machine's owner wrote that exact path down.
#[derive(Debug, Clone, Default)]
pub struct PrivilegedHelpers {
    allowed: Vec<String>,
}

impl PrivilegedHelpers {
    /// Build the list, refusing an entry that is not an absolute path.
    ///
    /// Relative entries are refused because they would be resolved against
    /// `PATH`, and a helper chosen by `PATH` is not a fixed list.
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
            if trimmed.contains("sudo") {
                return Err(format!(
                    "the privileged helper {trimmed:?} names sudo: a runner escalates \
                     through the fixed list itself, never through a general-purpose \
                     escalation tool (E-46)"
                ));
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

    /// Rule 2 is a rule about the list too: a list containing sudo is a list
    /// that allows everything.
    #[test]
    fn sudo_may_not_be_a_helper() {
        let error = PrivilegedHelpers::new(vec!["/run/wrappers/bin/sudo".to_owned()])
            .expect_err("sudo is not a fixed list");
        assert!(error.contains("sudo"), "{error}");
    }

    #[test]
    fn only_a_listed_helper_resolves_and_a_refusal_names_the_command() {
        let helpers = PrivilegedHelpers::new(vec![
            "/run/current-system/sw/bin/mount".to_owned(),
            "/run/current-system/sw/bin/mount".to_owned(),
        ])
        .expect("a valid list");
        assert_eq!(helpers.allowed().len(), 1, "a repeated entry is one entry");
        assert_eq!(
            helpers.resolve("/run/current-system/sw/bin/mount"),
            Ok("/run/current-system/sw/bin/mount")
        );
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
