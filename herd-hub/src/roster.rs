//! Which runners this hub has approved, and the hashes of their tokens
//! (E-48, Q8).
//!
//! One file in the state root, `runners.json`, written through
//! [`crate::durable`] like everything else the hub keeps (D8): no database,
//! and no second way to persist. It carries one entry per approved runner —
//! the name an operator approved, when, a random salt, and
//! `sha256(salt || token)`.
//!
//! **The token itself is never written anywhere.** It exists twice: in the
//! line `herdctl runner approve` prints, and in the configuration of the
//! machine that is to carry it. The hub keeps a hash, so a state root that
//! leaks — a backup, a volume someone copied, a `cat` in the wrong terminal —
//! leaks no credential (invariant 7). An operator who loses the line approves
//! the runner again and gets a new token; there is no "show me the token"
//! because there is nothing to show.
//!
//! **Why a plain digest and not a password hash.** A token here is 32 bytes
//! this hub took from the kernel's random source, not something a human
//! chose, so there is no small space of likely values for an attacker to walk
//! and nothing for an argon2 to slow down. The salt is still there, so that
//! two runners approved with the same token — which cannot happen by accident
//! and could by mistake — do not share a hash, and so that a hash says
//! nothing about any other hub's.
//!
//! An **empty roster refuses every runner**, and that is the default on a
//! state root nobody has approved anything on. A hub that served unknown
//! machines until an operator remembered to lock it down would be a hub whose
//! safe state is the one nobody reaches.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::durable;
use crate::job::{Result, StoreError};

/// The version of `runners.json`'s shape. A roster written by a newer Herd is
/// refused rather than read as if it were this one — the same rule the job
/// records live by, and here it decides who may connect.
pub const ROSTER_VERSION: u32 = 1;

const ROSTER_FILE: &str = "runners.json";

/// How many random bytes a token carries. 32, because this is the only thing
/// standing between a machine on the network and a runner's place in the
/// fleet, and because there is no reason to be cheap about bytes nobody
/// types from memory.
const TOKEN_BYTES: usize = 32;

/// How many random bytes salt one entry.
const SALT_BYTES: usize = 16;

/// What every token this hub mints begins with, so that one found in a
/// configuration file, an environment or a log is recognisable for what it
/// is — and so that a human who is about to paste it somewhere public has a
/// chance of noticing.
const TOKEN_PREFIX: &str = "herd-runner-";

/// `runners.json`.
#[derive(Debug, Serialize, Deserialize)]
struct RosterFile {
    roster_version: u32,
    runners: Vec<Entry>,
}

/// One approved runner, as the file carries it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    name: String,
    /// `sha256(salt || token)`, lowercase hex. Never the token.
    token_sha256: String,
    /// The salt, lowercase hex.
    salt: String,
    approved_unix_s: u64,
}

/// What an approved runner looks like to an operator (E-48).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub name: String,
    pub approved_unix_s: u64,
}

/// Whether a peer announcing a name and a token may be served, and if not,
/// which of the three ways it may not be.
///
/// Three nos rather than one, because they are three different things for a
/// person to do: approve a machine, hand a machine the token that was minted
/// for it, or find out why the token it has is not that one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The name is on the roster and the token matches its hash.
    Approved,
    /// No runner of that name has ever been approved here, or it was removed.
    NotApproved,
    /// Approved, and the handshake carried no token at all.
    NoToken,
    /// Approved, and the token is not the one that approval minted.
    WrongToken,
}

/// The approved runners of one state root.
///
/// Read once when the state root opens and written through on every change,
/// so a hub never answers a handshake from the disk and never leaves the disk
/// behind. The mutex is this type's own: approving a runner is rare and the
/// handshake only reads.
#[derive(Debug)]
pub struct Roster {
    path: PathBuf,
    entries: Mutex<BTreeMap<String, Entry>>,
}

impl Roster {
    /// Read the roster of the state root at `root`, or start an empty one.
    ///
    /// A root with no `runners.json` has approved nobody, which is the state
    /// a fresh deployment is in: every runner is refused until an operator
    /// approves one. A roster that will not parse, or whose version this hub
    /// does not write, is an error and not an empty roster — "this hub cannot
    /// read who is allowed here" must never degrade into "nobody is, so
    /// everything is refused for a reason that names the wrong cause".
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join(ROSTER_FILE);
        let mut entries = BTreeMap::new();
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| StoreError::new(format!("reading {}: {error}", path.display())))?;
            let found: RosterFile = serde_json::from_str(&text)
                .map_err(|error| StoreError::new(format!("reading {}: {error}", path.display())))?;
            if found.roster_version != ROSTER_VERSION {
                return Err(StoreError::new(format!(
                    "{} has roster version {}, and this hub writes version {ROSTER_VERSION}",
                    path.display(),
                    found.roster_version
                )));
            }
            for entry in found.runners {
                entries.insert(entry.name.clone(), entry);
            }
        }
        Ok(Self {
            path,
            entries: Mutex::new(entries),
        })
    }

    /// Approve `name`, mint its token, and keep only the hash (E-48).
    ///
    /// Answers with the token, which is the one time it is readable. A name
    /// that is already approved is approved again with a **new** token, and
    /// the old one stops working the moment this returns: that is how a token
    /// that has leaked is replaced, and it is why there is no second command
    /// for rotating one.
    ///
    /// The file is written before the token is handed back. An operator who
    /// was given a token the hub then failed to record would configure a
    /// machine that is refused for a reason nothing explains.
    pub fn approve(&self, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::new(
                "a runner is approved by the name it announces, and no name was given: \
                 `herdctl runner approve <name>`"
                    .to_owned(),
            ));
        }
        let token = format!("{TOKEN_PREFIX}{}", hex(&random_bytes(TOKEN_BYTES)?));
        let salt = random_bytes(SALT_BYTES)?;
        let entry = Entry {
            name: name.to_owned(),
            token_sha256: hex(&digest(&salt, &token)),
            salt: hex(&salt),
            approved_unix_s: now_unix_s(),
        };
        let mut entries = self.lock();
        let previous = entries.insert(name.to_owned(), entry);
        match self.write(&entries) {
            Ok(()) => Ok(token),
            Err(error) => {
                // The roster in memory is what the handshake reads, so a
                // write that failed must not leave this hub serving a token
                // nothing on disk remembers: the previous state goes back.
                match previous {
                    Some(entry) => entries.insert(name.to_owned(), entry),
                    None => entries.remove(name),
                };
                Err(error)
            }
        }
    }

    /// Remove `name` and revoke its token. `true` if it was there.
    pub fn remove(&self, name: &str) -> Result<bool> {
        let name = name.trim();
        let mut entries = self.lock();
        let Some(removed) = entries.remove(name) else {
            return Ok(false);
        };
        match self.write(&entries) {
            Ok(()) => Ok(true),
            Err(error) => {
                entries.insert(name.to_owned(), removed);
                Err(error)
            }
        }
    }

    /// Who is approved, by name, oldest approval first among equal names.
    pub fn approvals(&self) -> Vec<Approval> {
        self.lock()
            .values()
            .map(|entry| Approval {
                name: entry.name.clone(),
                approved_unix_s: entry.approved_unix_s,
            })
            .collect()
    }

    /// How many runners are approved, for the line the hub prints as it
    /// starts.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether this name and this token may be served (E-48).
    pub fn admit(&self, name: &str, token: Option<&str>) -> Admission {
        let entries = self.lock();
        let Some(entry) = entries.get(name.trim()) else {
            return Admission::NotApproved;
        };
        let Some(token) = token else {
            return Admission::NoToken;
        };
        let Some(salt) = unhex(&entry.salt) else {
            // A salt that will not read cannot be matched against, and
            // guessing one would admit a runner on a comparison nobody made.
            return Admission::WrongToken;
        };
        if same_bytes(
            hex(&digest(&salt, token)).as_bytes(),
            entry.token_sha256.as_bytes(),
        ) {
            Admission::Approved
        } else {
            Admission::WrongToken
        }
    }

    fn write(&self, entries: &BTreeMap<String, Entry>) -> Result<()> {
        let file = RosterFile {
            roster_version: ROSTER_VERSION,
            runners: entries.values().cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| StoreError::new(format!("encoding the roster: {error}")))?;
        durable::write_file(&self.path, &bytes)
            .map_err(|error| StoreError::new(format!("writing {}: {error}", self.path.display())))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Entry>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A shared secret the hub holds in memory only: the requester token it was
/// started with (Q8).
///
/// It is given as a **file path** rather than a value, so it never appears in
/// the hub's command line where any user of the host can read it out of
/// `/proc` (invariant 7). What is kept here is a salted hash of it, so a core
/// dump or a `Debug` of the hub's configuration carries no credential.
pub struct SharedSecret {
    salt: Vec<u8>,
    sha256: String,
}

impl SharedSecret {
    /// Hash `token` under a fresh random salt.
    pub fn of(token: &str) -> Result<Self> {
        let salt = random_bytes(SALT_BYTES)?;
        Ok(Self {
            sha256: hex(&digest(&salt, token)),
            salt,
        })
    }

    pub fn matches(&self, token: &str) -> bool {
        same_bytes(
            hex(&digest(&self.salt, token)).as_bytes(),
            self.sha256.as_bytes(),
        )
    }
}

/// Says that there is one and never what it is.
impl std::fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedSecret(<set>)")
    }
}

/// `sha256(salt || token)`.
fn digest(salt: &[u8], token: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(token.as_bytes());
    hasher.finalize().into()
}

/// Bytes from the kernel, or an error naming the host.
///
/// **No fallback to the clock**, unlike the state root's identity: a weak
/// state root id is a weak comparison, and a weak token is a credential
/// anyone who knows when the hub started can guess. A host that will not give
/// random bytes cannot mint a token, and says so.
fn random_bytes(how_many: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; how_many];
    let mut filled = 0;
    while filled < how_many {
        match rustix::rand::getrandom(&mut bytes[filled..], rustix::rand::GetRandomFlags::empty()) {
            Ok(0) => {
                return Err(StoreError::new(
                    "this host gave no random bytes for a runner's token, and a token this hub \
                     made up from its clock would be one an attacker could make up too"
                        .to_owned(),
                ))
            }
            Ok(some) => filled += some,
            Err(error) => {
                return Err(StoreError::new(format!(
                    "this host would not give random bytes for a runner's token ({error}), and \
                     a token this hub made up from its clock would be one an attacker could \
                     make up too"
                )))
            }
        }
    }
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len() / 2)
        .map(|n| u8::from_str_radix(&text[n * 2..n * 2 + 2], 16).ok())
        .collect()
}

/// Compare without letting the time taken say how far the two agreed.
///
/// The thing compared here is a hash of a 32-byte random token, so a timing
/// oracle on it buys an attacker very little. It costs one loop to not have
/// the question.
fn same_bytes(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

fn now_unix_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the whole milestone rests on: what lands on the disk does
    /// not contain the token.
    #[test]
    fn the_token_is_not_in_the_file_the_hub_writes() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let roster = Roster::open(dir.path()).expect("an empty roster opens");
        let token = roster.approve("wsl-pc").expect("the approval is minted");
        let written = std::fs::read_to_string(dir.path().join(ROSTER_FILE)).expect("it reads");
        assert!(
            !written.contains(&token),
            "the token is readable in the clear in {written}"
        );
        // And not in pieces either: the body past the prefix is what matters.
        let secret = token.trim_start_matches(TOKEN_PREFIX);
        assert!(
            !written.contains(secret),
            "the token's bytes are in {written}"
        );
        assert!(written.contains("wsl-pc"), "the name is not in {written}");
        assert_eq!(roster.admit("wsl-pc", Some(&token)), Admission::Approved);
    }

    #[test]
    fn a_roster_read_again_still_admits_the_token_it_minted() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let token = {
            let roster = Roster::open(dir.path()).expect("an empty roster opens");
            roster.approve("wsl-pc").expect("approved")
        };
        let reopened = Roster::open(dir.path()).expect("the roster reads back");
        assert_eq!(reopened.admit("wsl-pc", Some(&token)), Admission::Approved);
        assert_eq!(reopened.admit("wsl-pc", None), Admission::NoToken);
        assert_eq!(
            reopened.admit("wsl-pc", Some("herd-runner-nonsense")),
            Admission::WrongToken
        );
        assert_eq!(
            reopened.admit("the-server", Some(&token)),
            Admission::NotApproved,
            "a token admits the one name it was minted for"
        );
    }

    #[test]
    fn approving_again_mints_a_new_token_and_forgets_the_old() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let roster = Roster::open(dir.path()).expect("an empty roster opens");
        let first = roster.approve("wsl-pc").expect("approved");
        let second = roster.approve("wsl-pc").expect("approved again");
        assert_ne!(first, second, "two approvals minted the same token");
        assert_eq!(roster.admit("wsl-pc", Some(&second)), Admission::Approved);
        assert_eq!(roster.admit("wsl-pc", Some(&first)), Admission::WrongToken);
    }

    #[test]
    fn removing_a_runner_revokes_its_token() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let roster = Roster::open(dir.path()).expect("an empty roster opens");
        let token = roster.approve("wsl-pc").expect("approved");
        assert!(roster.remove("wsl-pc").expect("removed"));
        assert_eq!(roster.admit("wsl-pc", Some(&token)), Admission::NotApproved);
        assert!(
            !roster
                .remove("wsl-pc")
                .expect("removing it twice is not an error"),
            "a runner that was not there cannot have been removed"
        );
        let written = std::fs::read_to_string(dir.path().join(ROSTER_FILE)).expect("it reads");
        assert!(
            !written.contains("wsl-pc"),
            "the entry is still in {written}"
        );
    }

    #[test]
    fn an_empty_roster_approves_nobody() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let roster = Roster::open(dir.path()).expect("an empty roster opens");
        assert!(roster.is_empty());
        assert_eq!(
            roster.admit("wsl-pc", Some("herd-runner-anything")),
            Admission::NotApproved
        );
    }

    #[test]
    fn a_roster_this_hub_cannot_read_is_an_error_and_not_an_empty_roster() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(
            dir.path().join(ROSTER_FILE),
            br#"{"roster_version":99,"runners":[]}"#,
        )
        .expect("written");
        let error = Roster::open(dir.path()).expect_err("a future roster is refused");
        assert!(error.to_string().contains("roster version 99"), "{error}");
    }

    #[test]
    fn a_shared_secret_matches_only_itself() {
        let secret = SharedSecret::of("a-requester-token").expect("hashed");
        assert!(secret.matches("a-requester-token"));
        assert!(!secret.matches("a-requester-token "));
        assert!(!secret.matches(""));
        assert_eq!(format!("{secret:?}"), "SharedSecret(<set>)");
    }
}
