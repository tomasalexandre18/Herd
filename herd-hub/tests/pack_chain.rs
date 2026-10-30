//! M11's `{in}`/`{out}` chain, end to end, against the real binaries as
//! processes: a declared command leaves a pack, the hub registers it, and a
//! **later** declared command is handed it under `{in}` and reads the bytes.
//
//! This is the obligation M12's rows kept naming as their blocker, and it is
//! asserted here rather than in a library test because every link in the
//! chain is a different program: `herdctl` reads the declaration out of the
//! requester's own checkout, the hub resolves a `needs` key against its
//! registry, the runner lays the pack out and the consumer's own tool reads
//! `{in}/<key>` off the disk. A test that called `PackStore::lay_out`
//! directly would prove the one link that was never in doubt.
//
//! **Both of D10's paths, in that order, and each with its own runners.**
//!
//! The first test is **one runner, deliberately**: D10's first path is the
//! same host, so every object is already in that runner's store, nothing
//! moves on any wire, `{in}/<key>` is hard links into the store, and the
//! figure the hub is told is `same_host` with **nought** network bytes. That
//! zero is the assertion D10's same-host clause rests on, and it is the whole
//! of the common case — the build and the test run of one commit land on one
//! machine because the matcher prefers a runner that holds what a job needs
//! (E-23).
//!
//! The second is **two runners and a real transfer** (E-12's cross-machine
//! half): a pack produced on one real binary, consumed on the other, the
//! objects going peer to peer on the holder's advertised mesh endpoint and
//! **no byte of them passing through the hub**. It has its own section at the
//! foot of this file, with the outbound-only fixture that proves its `Peer`
//! can fail. The *push* direction — a holder that listens on nothing pushing
//! to an asker instead — is still the step after this one (ORCH-220).
//
//! What the first test asserts, in the order the chain happens:
//
//! * before anything is registered, `herdctl would-run` on the consumer says
//!   it **would be claimed and then wait for its packs** (D261) — the one
//!   answer a dry run of a job with `needs` could most easily get wrong, and
//!   the sentence this step had to not turn into a lie;
//! * the producer's pack leaves `{out}`, is **absent from its record**
//!   (ORCH-153, D223) and is registered with the hub (E-17);
//! * the consumer is laid out and **reads the bytes**, which is the only
//!   proof that matters: its own tool exits non-zero if `{in}/<key>` is
//!   empty or short of an object;
//! * the transfer is reported `same_host` with zero bytes off the network;
//! * and a key nothing registers is a job that **waits by name** and is
//!   never a green-looking job with an empty `{in}` (D150).

use std::path::{Path, PathBuf};
use std::process::Stdio;

mod real_herd;

use real_herd::Herd;

/// The branch the fixture checkout's commits are on.
const BRANCH: &str = "main";
/// The remote `fetch` fetches from, as the declaration names it.
const REMOTE: &str = "origin";
/// Where the declaration lives in a requester's checkout.
const DECLARATION_PATH: &str = "tools/herd/commands.toml";
/// Where the fixture consumer's one tool lives.
const STEP_SCRIPT: &str = "tools/herd/step.sh";

/// The producer: the command that leaves a pack.
const FORGE: &str = "forge";
/// The consumer: the command that needs it.
const GRAFT: &str = "graft";
/// A consumer of a key nothing ever registers.
const GRAFT_NOTHING: &str = "graft-nothing";
/// The `leaves` key the producer declares and the consumer needs.
const PACK_KEY: &str = "grain";

/// The producer and the consumer of the **two-machine** chain, each labelled
/// so that it can land on exactly one of two runners (E-23, E-25).
///
/// Labels and not luck: the hub's matcher prefers a runner that already holds
/// what a job needs, which is the right preference and the one that would
/// quietly turn a two-runner transfer back into a same-host layout. A label
/// only one runner offers is the only way to say "this job runs *there*"
/// without a requester naming a machine, which invariant 6 forbids.
const FORGE_HERE: &str = "forge-here";
const GRAFT_THERE: &str = "graft-there";

/// The declaration this checkout holds.
///
/// Every name in it is invented, like `herdctl`'s own fixture's: Herd's tests
/// name no consumer command (E-11). What is borrowed is the shape — an
/// executor of four hooks, and one table per command.
///
/// `forge` leaves `grain` at the directory `grain` under `{out}`; `graft`
/// needs `grain` and its line reads `{in}/grain` the way the consumer's own
/// `--pack DIR` would. No `labels`, because what is asserted here is the
/// chain and not the placement, and the placement has its own suite.
const DECLARATION: &str = r#"# The commands Herd may run for this repository.

[executor]
up = "mkdir -p '{job}' && printf 'up\n' >> '{job}/steps.txt'"
fetch = "cd '{job}' && git init -q . && git fetch -q --no-tags \"$GROVE_REMOTE\" '+refs/heads/*:refs/remotes/origin/*' && git checkout -q -f {commit} && printf 'fetch %s\n' '{commit}' >> steps.txt"
run = "cd '{job}' && printf 'run\n' >> steps.txt && {line}"
down = "printf 'down\n' >> '{job}/steps.txt'"

[executor.env]
GROVE_COMMIT = "{commit}"
GROVE_IN = "{in}"
GROVE_OUT = "{out}"
GROVE_REMOTE = "@REMOTE@"

[commands.forge]
line = "sh tools/herd/step.sh forge --leave-pack grain"
timeout_s = 600
leaves = { "grain" = "grain" }

[commands.graft]
line = "sh tools/herd/step.sh graft --read-pack \"$GROVE_IN/grain\""
timeout_s = 600
needs = ["grain"]

[commands.graft-nothing]
line = "sh tools/herd/step.sh graft-nothing"
timeout_s = 600
needs = ["chaff"]

[commands.forge-here]
line = "sh tools/herd/step.sh forge --leave-pack grain"
timeout_s = 600
labels = ["@HOLDS@"]
leaves = { "grain" = "grain" }

[commands.graft-there]
line = "sh tools/herd/step.sh graft --read-pack \"$GROVE_IN/grain\""
timeout_s = 600
labels = ["@NEEDS@"]
needs = ["grain"]
"#;

/// The fixture consumer's one tool: it writes a pack, or reads one.
///
/// A pack is three parts — `manifest.tsv`, `pack.meta` and
/// `objects/<sha256>` — and the manifest's bytes **are** the pack's identity,
/// so this writes them with `printf` and `sha256sum` and never through
/// anything of Herd's. Three rows and not one: the `kind f` row is the only
/// one whose sha256 names bytes under `objects/`, and the `d` and `l` rows
/// carry a sha256 of something that is not a file's bytes. A completeness
/// check without the `kind f` filter would look those two up, find nothing,
/// and report a perfectly good pack as incomplete — so the fixture carries
/// them and the filter is proven rather than assumed.
///
/// `--read-pack` exits **69** when the directory it was handed holds no
/// manifest or is short of an object, which is what makes the consumer's own
/// exit code the proof that the layout really happened: an empty `{in}` is
/// not a passing job.
///
/// ## The object's **second** line, and why it is there
///
/// The object carries a line nothing ever prints: `--read-pack` echoes the
/// first line only ([`what_the_pack_says`]) and the second is read by
/// [`only_inside_the_object`] alone. That makes it a needle that exists in
/// exactly one place — the pack's own bytes, on the two runners' disks and on
/// the wire between them — so a scan of everything the **hub** holds can say
/// that no byte of a pack passed through it (D10: the hub is registry and
/// authority, never the byte path). Without it the needle would be the first
/// line, which the consumer's own tool prints into a job log the hub keeps,
/// and the scan would fire on evidence of its own making.
const STEP_SOURCE: &str = r#"#!/bin/sh
set -u
step=${1:?a step name}
shift
leave_pack=
read_pack=
previous=
for word in "$@"; do
    case $previous in
    --leave-pack) leave_pack=$word ;;
    --read-pack) read_pack=$word ;;
    esac
    previous=$word
done
printf 'grove %s: %s\n' "$step" "$*"
if [ -n "$leave_pack" ]; then
    out=${GROVE_OUT:?--leave-pack needs the output directory in GROVE_OUT}
    dir=$out/$leave_pack
    mkdir -p "$dir/objects"
    printf 'grove pack of %s\n' "${GROVE_COMMIT:-no commit}" > "$dir/bytes"
    printf 'grove-object-only %s\n' "${GROVE_COMMIT:-no commit}" >> "$dir/bytes"
    sha=$(sha256sum < "$dir/bytes" | cut -d' ' -f1)
    size=$(wc -c < "$dir/bytes")
    mv "$dir/bytes" "$dir/objects/$sha"
    {
        printf '# chatons pack 1\n'
        printf '# profile debug\n'
        printf '%s\t%s\t0644\tf\tartefact\tgrove.txt\n' "$sha" "$size"
        printf '1111111111111111111111111111111111111111111111111111111111111111\t0\t0755\td\tartefact\tnested\n'
        printf '2222222222222222222222222222222222222222222222222222222222222222\t0\t0777\tl\tartefact\tnested/latest\tgrove.txt\n'
    } > "$dir/manifest.tsv"
    printf 'commit %s\n' "${GROVE_COMMIT:-no commit}" > "$dir/pack.meta"
    printf 'grove %s: left the pack %s, one object %s\n' "$step" "$leave_pack" "$sha"
fi
if [ -n "$read_pack" ]; then
    if [ ! -f "$read_pack/manifest.tsv" ]; then
        printf 'grove %s: no pack at %s\n' "$step" "$read_pack" >&2
        exit 69
    fi
    for sha in $(awk -F'\t' '$4 == "f" { print $1 }' "$read_pack/manifest.tsv"); do
        if [ ! -f "$read_pack/objects/$sha" ]; then
            printf 'grove %s: %s is short of object %s\n' "$step" "$read_pack" "$sha" >&2
            exit 69
        fi
        printf 'grove %s: read %s' "$step" "$(head -n 1 "$read_pack/objects/$sha")"
        printf '\n'
    done
    printf 'grove %s: the pack says %s\n' "$step" "$(cat "$read_pack/pack.meta")"
fi
printf 'grove %s: verdict 0\n' "$step"
"#;

/// The one line the consumer's tool prints when it has really read the pack's
/// bytes out of `{in}/<key>/objects/<sha256>`.
fn what_the_pack_says(commit: &str) -> String {
    format!("grove graft: read grove pack of {commit}")
}

/// The one string that exists **only** inside the pack's object bytes.
///
/// Nothing prints it: `--read-pack` echoes the object's first line and this
/// is its second (see [`STEP_SOURCE`]). So it is in exactly three places —
/// the producing runner's store, the consuming runner's store, and the socket
/// between them — and in none of the hub's, which is what
/// [`nothing_of_the_pack_is_on_the_hub`] measures.
fn only_inside_the_object(commit: &str) -> String {
    format!("grove-object-only {commit}")
}

/// A requester's checkout: a bare remote, a work tree holding the declaration
/// and the tool, committed and pushed.
///
/// The push is not a formality: Herd hands a runner a hash and holds no
/// repository, so the `fetch` hook clones from this remote and a commit that
/// is not on it cannot be run at all (E-01, E-08).
struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    /// `None` means this host has no `git` or no `sha256sum`, and the test has
    /// skipped loudly under its own name (D27). A caller returns on `None`; it
    /// must never read it as a chain that worked.
    fn new(test: &str) -> Option<Checkout> {
        for tool in ["git", "sha256sum", "awk", "head"] {
            if !on_the_path(tool) {
                herd_runner::testing::skipped(
                    test,
                    &format!(
                        "{tool} is not on this host's PATH, so no fixture consumer repository \
                         could be built and nothing about a pack chain could be read"
                    ),
                );
                return None;
            }
        }
        let dir = tempfile::tempdir().expect("a temporary directory for the checkout");
        for made in ["remote", "work"] {
            std::fs::create_dir_all(dir.path().join(made)).expect("the checkout's directories");
        }
        let checkout = Checkout { dir };
        checkout.git_in(
            "remote",
            &["init", "-q", "--bare", "--initial-branch", BRANCH],
        );
        checkout.git(&["init", "-q", "--initial-branch", BRANCH, "."]);
        let remote = checkout.remote();
        checkout.write(
            DECLARATION_PATH,
            &DECLARATION
                .replace("@REMOTE@", &remote.to_string_lossy())
                // The two labels are written once, where the runners that
                // offer them are named, and substituted here: a label typed
                // twice is a job that waits for a machine that never offers
                // it, and the wait looks exactly like a hub that is broken.
                .replace("@HOLDS@", HOLDS)
                .replace("@NEEDS@", NEEDS),
        );
        checkout.write(STEP_SCRIPT, STEP_SOURCE);
        checkout.git(&["add", "-A"]);
        checkout.git(&["commit", "-qm", "declare the commands Herd may run"]);
        checkout.git(&["remote", "add", REMOTE, &remote.to_string_lossy()]);
        checkout.git(&["push", "-q", "--set-upstream", REMOTE, BRANCH]);
        // Established and not assumed: every test here stands on the commit
        // being on a remote-tracking ref, because that is what `herdctl`
        // refuses a submission for (E-08).
        let head = checkout.rev_parse("HEAD");
        let holding = checkout.git(&["branch", "--remotes", "--contains", &head]);
        assert!(
            holding.contains(&format!("{REMOTE}/{BRANCH}")),
            "the checkout pushed {head} and no remote-tracking ref holds it: {holding:?}"
        );
        Some(checkout)
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn remote(&self) -> PathBuf {
        self.dir.path().join("remote")
    }

    fn rev_parse(&self, named: &str) -> String {
        self.git(&["rev-parse", named])
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the file's directory");
        }
        std::fs::write(path, text).expect("the checkout writes a file");
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in("work", args)
    }

    fn git_in(&self, dir: &str, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(self.dir.path().join(dir))
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.dir.path())
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Herd Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@herd.invalid")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_NAME", "Herd Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@herd.invalid")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
            .stdin(Stdio::null())
            .output()
            .expect("git runs in the checkout");
        assert!(
            output.status.success(),
            "the checkout's own git failed in {dir}: git {}\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

/// Whether a tool is on this host's PATH.
///
/// Asked with `command -v` and not with `--version`: a tool that is there but
/// does not take that option — `awk` on more than one host — would be read as
/// absent, and the test would skip for a reason that is not true. A skip that
/// names the wrong reason is worse than a failure (D27).
fn on_the_path(tool: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {tool}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// A hub and one runner, both real processes, with a checkout to submit from.
///
/// Both processes are handed back and **both must be held for the length of
/// the test**: `Process` kills its child when it is dropped, which is how
/// this suite leaves no process behind on a shared host.
async fn a_herd_and_a_checkout(
    test: &str,
) -> Option<(Herd, Checkout, real_herd::Process, real_herd::Process)> {
    let checkout = Checkout::new(test)?;
    let herd = Herd::new();
    let hub = herd.start_hub("hub").await;
    let runner = herd.start_runner();
    herd.wait_for_runner().await;
    Some((herd, checkout, hub, runner))
}

fn status_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .unwrap_or_else(|| panic!("herdctl itself was killed: {:?}", output.status))
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// **The chain, whole** (E-12, E-17, D10, ORCH-153, ORCH-274).
///
/// One test and not four, because what is being asserted is that the links
/// join: a producer whose pack is registered and a consumer that is laid out
/// are two halves of one sentence, and split across two tests each half could
/// pass while the pair did not.
#[tokio::test]
async fn a_declared_command_leaves_a_pack_and_a_later_one_is_laid_out_with_it() {
    let test = "a_declared_command_leaves_a_pack_and_a_later_one_is_laid_out_with_it";
    let Some((herd, checkout, _hub, mut runner)) = a_herd_and_a_checkout(test).await else {
        return;
    };
    let commit = checkout.rev_parse(BRANCH);

    // ---------------------------------------------------------------- D261
    // Before anything is registered: the dry run says the dispatch would
    // hold this job back, and does not say it would start now. This is the
    // sentence M12 L3 landed and the one this step had to not make a lie —
    // and it is reached through `herdctl`, which is the half ORCH-274 was
    // about: with `needs` dropped on the way to the wire, this answer was
    // unreachable from this CLI at all.
    let dry = herd
        .herdctl_in(
            &checkout.root(),
            &["would-run", "--command", GRAFT, "--commit", BRANCH],
        )
        .await;
    assert_eq!(
        status_code(&dry),
        0,
        "a dry run of a job that would wait is not an error: {}",
        stderr(&dry)
    );
    let said = stdout(&dry);
    assert!(
        said.contains("it would be claimed and then wait for its packs"),
        "the dry run must say the dispatch is what holds it back (D261): {said}"
    );
    assert!(
        said.contains(&format!("nothing is registered for {PACK_KEY} at {commit}")),
        "and name the key and the commit it could not be resolved against: {said}"
    );
    assert!(
        !said.contains("it would start now"),
        "a job the dispatch would hold back does not start now (E-07): {said}"
    );

    // ------------------------------------------------------------- the producer
    let forged = herd
        .herdctl_in(
            &checkout.root(),
            &["run", "--command", FORGE, "--commit", BRANCH],
        )
        .await;
    assert_eq!(
        status_code(&forged),
        0,
        "the producer must run: {}\n{}",
        stderr(&forged),
        herd.everything_said()
    );
    let said = stdout(&forged);
    assert!(
        said.contains("left the pack grain"),
        "the producer's own tool must say it wrote one: {said}"
    );

    // Registered, said by the runner in its own voice — and said as
    // registered rather than as held, which is the half ORCH-260 named: the
    // store had the pack and nothing constructed `RegisterPack`.
    // The positive wording and not a substring of the negative one: "NOT
    // registered with the hub" contains "registered with the hub", so a wait
    // on the shorter phrase is satisfied by the sentence that says the
    // opposite. Measured: with the registration removed, that wait passed and
    // only the assertion below caught it.
    herd.wait_until("the runner says it registered the pack", || {
        herd.said("runner")
            .contains("pack store and registered with the hub")
    })
    .await;
    assert!(
        !herd
            .said("runner")
            .contains("is NOT registered with the hub"),
        "and not as a pack it could not register: {}",
        herd.said("runner")
    );
    herd.wait_until("the hub says it holds the pack", || {
        herd.said("hub").contains(&format!("{commit}/{PACK_KEY}"))
    })
    .await;

    // ORCH-153, D223: the pack is **gone** from `{out}` before the record
    // walk, so the record carries none of its files. Read off the record as a
    // reader outside Herd would (E-15).
    let records = every_record(&herd);
    let producer = records
        .iter()
        .find(|record| name_of(record) == Some(FORGE.to_owned()))
        .unwrap_or_else(|| panic!("no record for the producer: {records:#?}"));
    let outputs = producer["outputs"]["files"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !outputs.iter().any(|file| file["path"]
            .as_str()
            .is_some_and(|path| path.starts_with(PACK_KEY))),
        "a pack must not reach a record as output evidence (ORCH-153): {outputs:#?}"
    );

    // ------------------------------------------------------------- the consumer
    let grafted = herd
        .herdctl_in(
            &checkout.root(),
            &["run", "--command", GRAFT, "--commit", BRANCH],
        )
        .await;
    assert_eq!(
        status_code(&grafted),
        0,
        "the consumer must be laid out and run: {}\n{}",
        stderr(&grafted),
        herd.everything_said()
    );
    let said = stdout(&grafted);
    // **The assertion the whole chain exists for**: the consumer's own tool
    // read the pack's bytes out of `{in}/<key>/objects/<sha256>`. It exits 69
    // when that directory holds no manifest or is short of an object, so the
    // exit code above already says the layout happened; this says the bytes
    // were the right ones.
    assert!(
        said.contains(&what_the_pack_says(&commit)),
        "the consumer must read the pack's own bytes out of {{in}}: {said}"
    );
    assert!(
        said.contains(&format!("the pack says commit {commit}")),
        "and its pack.meta, which rides the control plane beside the manifest: {said}"
    );

    // D10's first path, in figures: the runner already held every object, so
    // nothing was asked of the hub, nothing was dialled, and **no byte moved
    // on any wire**. The zero is the assertion the same-host clause rests on.
    herd.wait_until("the hub says what the transfer cost", || {
        herd.said("hub").contains("moved pack")
    })
    .await;
    let hub_said = herd.said("hub");
    let moved = hub_said
        .lines()
        .find(|line| line.contains("moved pack"))
        .unwrap_or_else(|| panic!("the hub said nothing about a transfer: {hub_said}"));
    assert!(
        moved.contains("SameHost") && moved.contains("0 byte(s)"),
        "the pack was already on this host, so the path is same-host and the \
         network bytes are nought (D10): {moved}"
    );
    assert!(
        moved.contains("1 object(s) fetched, 0 already held")
            || moved.contains("0 object(s) fetched, 1 already held"),
        "and the figures must say which: {moved}"
    );
    // The runner's own line, for the owner of the machine.
    assert!(
        herd.said("runner")
            .contains(&format!("is laid out for {PACK_KEY}")),
        "the runner must say which key it laid out and by which path: {}",
        herd.said("runner")
    );

    // ----------------------------------------------------- a key nobody leaves
    // D150's failure, refused: a job needing a key nothing registers waits
    // and is told why, and is never a green-looking job with an empty
    // `{in}`. Asserted through a dry run rather than a submission, because a
    // job that waits for ever is a job `herdctl run` waits for ever on —
    // which is the right behaviour and a bad test.
    let nothing = herd
        .herdctl_in(
            &checkout.root(),
            &["would-run", "--command", GRAFT_NOTHING, "--commit", BRANCH],
        )
        .await;
    let said = stdout(&nothing);
    assert!(
        said.contains("it would be claimed and then wait for its packs")
            && said.contains("nothing is registered for chaff"),
        "a key nothing leaves is a wait that names the key, and never a \
         silence (D150): {said}"
    );

    // Nothing here depended on a runner that died halfway.
    assert!(
        runner.alive(),
        "the runner must still be running: {}",
        herd.said("runner")
    );
    runner.ask_it_to_stop();
}

/// Every job record on the hub's disk, read as a reader outside Herd would
/// (E-15).
fn every_record(herd: &Herd) -> Vec<serde_json::Value> {
    let jobs = herd.state.path().join("jobs");
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(&jobs) {
        Ok(entries) => entries
            .map(|entry| entry.expect("a job directory").path())
            .collect(),
        Err(_) => Vec::new(),
    };
    dirs.sort();
    dirs.iter().filter_map(|dir| read_record(dir)).collect()
}

fn read_record(dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join("job.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Which declared command a record is of, when it is of one.
fn name_of(record: &serde_json::Value) -> Option<String> {
    record["declaration"]["command"]
        .as_str()
        .map(ToOwned::to_owned)
}

// ------------- the hub authorises a transfer, and logs no secret ----

// What follows is the **authorisation** half of the chain, against the real
// hub *process*. It is here rather than in `pack_frames.rs` for one reason:
// the property under test is what the hub **writes down**, and a hub running
// inside the test binary writes its journal to the test binary's own file
// descriptor 2, where no test can read it. A hub started as a process has
// its stderr in a file, which is the only place this assertion can be made.

/// The runner that holds the pack, and the label only it offers.
const HOLDER: &str = "the-holder";
const HOLDS: &str = "holds-packs";
/// The runner that needs the objects, and the label only it offers.
const ASKER: &str = "the-asker";
const NEEDS: &str = "needs-packs";
/// Where the holder says it serves content: an address on a private mesh
/// interface, the only kind invariant 3 permits a runner to bind (D11).
const HOLDER_ENDPOINT: &str = "10.42.0.7:51820";
/// The commit and key the fixture pack is registered under.
const AUTH_COMMIT: &str = "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68";
const AUTH_KEY: &str = "grain-authorised";

/// A one-object manifest. The bytes **are** the pack's identity, so this is
/// written out rather than built, exactly as the store's own shape has it.
fn an_authorised_manifest() -> Vec<u8> {
    format!(
        "# chatons pack 1\n# profile debug\n{}\t4096\t0644\tf\tartefact\tgrain.txt\n",
        "7".repeat(64)
    )
    .into_bytes()
}

fn the_authorised_pack() -> herd_proto::PackId {
    herd_proto::PackId::of_manifest_bytes(&an_authorised_manifest())
}

/// A runner driven frame by frame against the real hub process.
///
/// Hand-written and not `herd-runner`, and the reason has changed since this
/// was written: the real binary now advertises the mesh endpoint it bound, so
/// it *can* be a holder — [`a_pack_moves_from_one_real_runner_to_another`] is
/// that chain, with both runners real. What a hand-driven peer still does
/// better is **refuse** on demand: the ten refusal paths of the hub's
/// `AskTransferCapability` arm are reached by sending a frame the real runner
/// would never send, and the assertion that the hub writes down no token
/// needs a capability in the test's own hands to search its journal for.
/// So the two coexist, and neither is the other's simplification.
struct Dialled {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl Dialled {
    /// `token` is passed in rather than minted here, because **the order
    /// matters**: a hub whose roster is empty refuses every submission by
    /// name — *"0 runner(s) are approved on this state root, so nothing
    /// submitted here could ever start"* — rather than queueing a job that
    /// would wait for nobody coming. So both names are approved before the
    /// first job is submitted, and this only dials.
    async fn joining(
        herd: &Herd,
        name: &str,
        token: &str,
        label: &str,
        endpoint: Option<&str>,
    ) -> Self {
        use futures_util::SinkExt;
        let token = token.to_owned();
        let (mut socket, _response) = tokio_tungstenite::connect_async(herd.url())
            .await
            .expect("the real hub accepts a runner");
        let offer = herd_proto::CapabilityOffer::new(vec![herd_proto::OfferedCapability::new(
            herd_proto::Capability::shared(label),
            herd_proto::CapabilityOrigins::declared(),
        )])
        .expect("a valid offer");
        let hello = herd_proto::ToHub::Hello {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            peer_version: herd_runner::runner_version().to_owned(),
            role: herd_proto::Role::Runner,
            token: Some(token),
            name: Some(name.to_owned()),
            instance: Some("the-only-life".to_owned()),
            state_root: None,
            health_every_ms: None,
            capabilities: offer,
            host: herd_proto::HostSize {
                memory_mib: Some(8192),
                threads: Some(4),
                memory_floor_mib: Some(1024),
                tmpfs_mib: Some(4096),
            },
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
            jobs_at_once: None,
            mesh_endpoint: endpoint.map(ToOwned::to_owned),
        };
        socket
            .send(tokio_tungstenite::tungstenite::Message::text(
                herd_proto::to_frame(&hello).expect("hello encodes"),
            ))
            .await
            .expect("hello is sent");
        let mut joined = Self { socket };
        match joined.next().await {
            Some(herd_proto::FromHub::Welcome { .. }) => joined,
            other => panic!("an approved runner is welcomed, got {other:?}"),
        }
    }

    async fn say(&mut self, message: herd_proto::ToHub) {
        use futures_util::SinkExt;
        self.socket
            .send(tokio_tungstenite::tungstenite::Message::text(
                herd_proto::to_frame(&message).expect("the frame encodes"),
            ))
            .await
            .expect("the frame is sent");
    }

    async fn next(&mut self) -> Option<herd_proto::FromHub> {
        use futures_util::StreamExt;
        loop {
            let frame = self.socket.next().await?.expect("the frame is readable");
            if frame.is_close() {
                return None;
            }
            let Ok(text) = frame.to_text() else { continue };
            if text.is_empty() {
                continue;
            }
            return Some(herd_proto::from_frame(text).expect("the hub speaks the protocol"));
        }
    }

    /// Wait for the job the hub assigns this runner, and keep it `assigned`:
    /// that is the window a fetch is asked in.
    async fn takes_its_job(&mut self) -> u32 {
        loop {
            match self.next().await {
                Some(herd_proto::FromHub::AssignJob { attempt, .. }) => return attempt,
                Some(_) => continue,
                None => panic!("the hub never handed this runner a job"),
            }
        }
    }
}

/// **The hub authorises a real transfer, and writes down no secret** (D209,
/// D200, invariant 7).
///
/// Two assertions a hub inside the test binary cannot carry, both about the
/// one sentence an operator reads:
///
/// * it names the **scope** — who may fetch what, from whom, at which
///   address, until when — because a transfer with no figures on it is not
///   something an operator can follow;
/// * and it carries **no part of the token**. A capability is a bearer
///   secret: its holder *is* its subject, so a token in a journal is a
///   journal that authorises a transfer to whoever reads it, and journals are
///   shipped, pasted and attached to reports.
///
/// Watched red: with the journal line changed to print the token, the second
/// assertion failed and named it; with the `ExpectTransfer` send removed, the
/// *first* test of this pair failed at the holder's frame and this one still
/// passed — which is why both exist.
#[tokio::test]
async fn the_hub_authorises_a_transfer_and_writes_no_token_down() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    // Both approvals first: see `Dialled::joining`.
    let holder_token = herd.approve(HOLDER);
    let asker_token = herd.approve(ASKER);

    // Two jobs, each labelled for exactly one of the two runners: with two
    // connected, an unlabelled job has two places it could be claimed.
    let holders_job_id = herd.submit_needing(HOLDS, "step", "true").await;
    let mut holder =
        Dialled::joining(&herd, HOLDER, &holder_token, HOLDS, Some(HOLDER_ENDPOINT)).await;
    let attempt = holder.takes_its_job().await;
    let holders_job = herd_proto::JobId::new(holders_job_id.clone());

    // Registered and announced through the frames, which are the only things
    // in this build that make a holder.
    holder
        .say(herd_proto::ToHub::RegisterPack(herd_proto::RegisterPack {
            job_id: holders_job.clone(),
            attempt,
            commit: AUTH_COMMIT.to_owned(),
            key: AUTH_KEY.to_owned(),
            manifest: an_authorised_manifest(),
            meta: None,
        }))
        .await;
    holder
        .say(herd_proto::ToHub::PacksHeld(herd_proto::PacksHeld {
            packs: vec![the_authorised_pack()],
            more: false,
        }))
        .await;
    herd.wait_until("the hub records the holding", || {
        herd.said("hub")
            .contains(&format!("{AUTH_COMMIT}/{AUTH_KEY}"))
    })
    .await;

    let askers_job = herd.submit_needing(NEEDS, "step", "true").await;
    let mut asker = Dialled::joining(&herd, ASKER, &asker_token, NEEDS, None).await;
    asker.takes_its_job().await;

    let wanted = vec!["7".repeat(64)];
    asker
        .say(herd_proto::ToHub::AskTransferCapability(
            herd_proto::AskTransferCapability {
                job_id: herd_proto::JobId::new(askers_job.clone()),
                pack: the_authorised_pack(),
                peer: Some(HOLDER.to_owned()),
                direction: herd_proto::TransferDirection::Fetch,
                objects: wanted,
            },
        ))
        .await;
    let capability = loop {
        match asker.next().await {
            Some(herd_proto::FromHub::TransferCapability(capability)) => break capability,
            Some(herd_proto::FromHub::PackRefused(refused)) => panic!(
                "the real hub refused a transfer it should have authorised: {}\n{}",
                refused.why,
                herd.everything_said()
            ),
            Some(_) => continue,
            None => panic!("the asker's link closed: {}", herd.everything_said()),
        }
    };
    assert_eq!(capability.endpoint, HOLDER_ENDPOINT);
    assert_eq!(capability.peer, HOLDER);

    // The one sentence an operator reads, with every figure on it.
    herd.wait_until("the hub says it authorised the transfer", || {
        herd.said("hub").contains("may fetch")
    })
    .await;
    let said = herd.said("hub");
    let line = said
        .lines()
        .find(|line| line.contains("may fetch"))
        .unwrap_or_else(|| panic!("the hub said nothing about the transfer: {said}"));
    assert!(
        line.contains(ASKER) && line.contains(HOLDER) && line.contains(HOLDER_ENDPOINT),
        "the line names who may fetch, from whom and where: {line}"
    );
    assert!(
        line.contains("1 object(s)"),
        "and the scope it authorised, which is not the whole pack: {line}"
    );

    // **The secret is not there.** Not the token, and not a prefix of it long
    // enough to be worth having: the whole random part is checked, and so is
    // every 16-character window of it, so a line that printed "the first few
    // characters for correlation" would fail here too.
    assert!(
        !said.contains(&capability.token),
        "a capability is a bearer secret and this hub wrote it into its own journal: {line}"
    );
    let random = capability
        .token
        .strip_prefix("herd-pack-")
        .expect("a transfer token says what it is");
    for start in 0..=random.len().saturating_sub(16) {
        let window = &random[start..start + 16];
        assert!(
            !said.contains(window),
            "{window} — sixteen characters of the token — is in the hub's journal: {line}"
        );
    }
}

// ------------- two machines, and the bytes between them -------------

// What follows is **E-12's cross-machine half**, against the real binaries:
// two real runners, a pack produced on one and consumed on the other, and the
// objects going peer to peer on the fleet's own mesh with nothing of them
// passing through the hub (D10).
//
// It is the same chain as the one-runner test above and deliberately not a
// replacement for it. D10 has two paths, the same host and a peer; the
// same-host path is the common case and its **nought** network bytes are the
// assertion that clause rests on. A file that asserted only the transfer
// would have stopped measuring the path that happens on nearly every job.

/// What the holder is told to listen on.
///
/// Loopback, and a bare address so that the **kernel** picks the port: this
/// is the spelling the runner's whole bind-before-the-dial sequencing exists
/// for, and the spelling that would have gone on silently half-working if the
/// advertisement had been limited to an explicit `ADDR:PORT`.
///
/// It is a deliberate clause of `herd-runner/src/mesh.rs` and not an
/// accident: `permitted` returns `Ok` for loopback before the routing table
/// is consulted at all, because it is reachable from nothing off this machine
/// and it is the only mesh a single workstation running this test has. **No
/// guard is weakened to make this address advertisable** (D11, D222,
/// invariant 3).
const BARE_LOOPBACK: &str = "127.0.0.1";

/// A hub and **two** real runners, one labelled to hold packs and one
/// labelled to need them, with a checkout to submit from.
///
/// `holder_mesh` is the holder's `--mesh-listen`, or `None` for the
/// outbound-only holder — which is the fixture that proves the transfer
/// assertion below can fail.
///
/// Every process is handed back and **all of them must be held for the length
/// of the test**: `Process` kills its child when it is dropped, which is how
/// this suite leaves nothing behind on a shared host.
async fn two_machines(
    test: &str,
    holder_mesh: Option<&str>,
) -> Option<(
    Herd,
    Checkout,
    real_herd::Process,
    real_herd::Process,
    real_herd::Process,
)> {
    let checkout = Checkout::new(test)?;
    let herd = Herd::new();
    let hub = herd.start_hub("hub").await;
    // `--no-detect` on both: what each machine offers is then exactly the one
    // invented label it was given, so which runner a job lands on is a fact
    // about the declaration and never about what this host happens to have.
    let mut holder_options = vec!["--no-detect", "--declare-capability", HOLDS];
    if let Some(mesh) = holder_mesh {
        holder_options.extend_from_slice(&["--mesh-listen", mesh]);
    }
    let holding = herd.start_runner_with(HOLDER, "holder", "holder", &holder_options);
    let asking = herd.start_runner_with(
        ASKER,
        "asker",
        "asker",
        &["--no-detect", "--declare-capability", NEEDS],
    );
    // Both, and each through its own label: `wait_for_runner` submits an
    // unlabelled job, which either machine could claim, so it would return
    // with one of the two still not joined — and the job that then waited
    // for the absent one would look exactly like a hub that had lost it.
    for label in [HOLDS, NEEDS] {
        let ready = herd
            .herdctl(&[
                "run",
                "--label",
                label,
                "--",
                "/bin/sh",
                "-c",
                "printf ready",
            ])
            .await;
        assert_eq!(
            status_code(&ready),
            0,
            "the runner offering {label} never joined: {}\n{}",
            stderr(&ready),
            herd.everything_said()
        );
    }
    Some((herd, checkout, hub, holding, asking))
}

/// Where `needle` turns up in anything the **hub** holds, if anywhere: its
/// journal, and every byte under its state root read as a reader outside Herd
/// would (E-15).
///
/// The hub keeps a manifest, a `pack.meta` and a job's log by design — the
/// metadata rides the control plane, which is D10's own clause — so what is
/// searched for is the one string that lives only in the pack's object bytes
/// ([`only_inside_the_object`]).
fn where_the_hub_holds(herd: &Herd, needle: &str) -> Option<String> {
    if herd.said("hub").contains(needle) {
        return Some("the hub's own journal".to_owned());
    }
    let wanted = needle.as_bytes();
    herd.state_bytes()
        .into_iter()
        .find(|(_, bytes)| bytes.windows(wanted.len()).any(|window| window == wanted))
        .map(|(path, _)| format!("the hub's state root, at {path}"))
}

/// **A pack moves from one real runner to another, peer to peer** (E-12, D10,
/// D11, D209, invariant 3).
///
/// The obligation E-12's cross-machine half exists for, and the one thing
/// nothing else in this workspace discharges: every link is a different
/// program, and the two runners are the real binary with its own startup, its
/// own bind and its own fetch.
///
/// Four assertions, and each is a separate promise:
///
/// * the consumer's **own tool** reads the pack's bytes out of `{in}/<key>`,
///   and exits 69 if that directory holds no manifest or is short of an
///   object — so a green job is proof of a layout and not of a wait;
/// * the transfer is reported **`Peer`** and not `SameHost`, which is the
///   whole point: the asker held none of it and dialled the holder's
///   advertised endpoint;
/// * the figure is **more than nought bytes**, because `SameHost` and a
///   failed transfer both report nought and only a positive figure says
///   something crossed a socket;
/// * and **no byte of the pack is anywhere on the hub** — not in its journal
///   and not under its state root. The hub is registry and authority and
///   never the byte path (D10), and that is asserted rather than assumed.
#[tokio::test]
async fn a_pack_moves_from_one_real_runner_to_another() {
    let test = "a_pack_moves_from_one_real_runner_to_another";
    let Some((herd, checkout, _hub, mut holder, mut asker)) =
        two_machines(test, Some(BARE_LOOPBACK)).await
    else {
        return;
    };
    let commit = checkout.rev_parse(BRANCH);

    // The holder advertised a real endpoint before it ever said hello. Read
    // off its own words, because this is the premise everything below rests
    // on: with `mesh_endpoint: None` the hub mints nothing, and the failure
    // further down would be a transfer refused rather than a transfer
    // misreported.
    herd.wait_until("the holder says what it advertises", || {
        herd.said("holder").contains("serves pack content at")
    })
    .await;
    let said = herd.said("holder");
    let advertised = said
        .lines()
        .find_map(|line| line.split("serves pack content at ").nth(1))
        .map(|at| at.trim().to_owned())
        .unwrap_or_else(|| panic!("the holder named no endpoint: {said}"));
    assert!(
        advertised.starts_with(&format!("{BARE_LOOPBACK}:")) && !advertised.ends_with(":0"),
        "the holder must advertise the port the kernel gave it and not the nought it asked \
         for: {advertised}"
    );
    // And the asker advertises nothing at all, which is what an outbound-only
    // runner is: it fetches by dialling out and needs nothing inbound
    // (invariant 3).
    assert!(
        herd.said("asker").contains("outbound-only"),
        "the asker must be outbound-only: {}",
        herd.said("asker")
    );

    // ------------------------------------------------- the producer, on A
    let forged = herd
        .herdctl_in(
            &checkout.root(),
            &["run", "--command", FORGE_HERE, "--commit", BRANCH],
        )
        .await;
    assert_eq!(
        status_code(&forged),
        0,
        "the producer must run on the holder: {}\n{}",
        stderr(&forged),
        herd.everything_said()
    );
    assert!(
        stdout(&forged).contains("left the pack grain"),
        "the producer's own tool must say it wrote one: {}",
        stdout(&forged)
    );
    // On the holder and **only** there: a label only one machine offers is
    // what puts it there, and if both had run it the fetch below would be a
    // same-host layout wearing a two-runner test's name.
    herd.wait_until("the holder says it registered the pack", || {
        herd.said("holder")
            .contains("pack store and registered with the hub")
    })
    .await;
    assert!(
        !herd
            .said("asker")
            .contains("pack store and registered with the hub"),
        "the pack was produced on the asker as well, so nothing here measures a transfer: {}",
        herd.said("asker")
    );
    herd.wait_until("the hub says it holds the pack", || {
        herd.said("hub").contains(&format!("{commit}/{PACK_KEY}"))
    })
    .await;

    // ------------------------------------------------- the consumer, on B
    let grafted = herd
        .herdctl_in(
            &checkout.root(),
            &["run", "--command", GRAFT_THERE, "--commit", BRANCH],
        )
        .await;
    assert_eq!(
        status_code(&grafted),
        0,
        "the consumer must be laid out from the other machine and run: {}\n{}",
        stderr(&grafted),
        herd.everything_said()
    );
    let said = stdout(&grafted);
    assert!(
        said.contains(&what_the_pack_says(&commit)),
        "the consumer must read the pack's own bytes out of {{in}}, which arrived over the \
         mesh: {said}"
    );
    assert!(
        said.contains(&format!("the pack says commit {commit}")),
        "and its pack.meta, which rides the control plane beside the manifest: {said}"
    );

    // D10's **second** path, in figures.
    herd.wait_until("the hub says what the transfer cost", || {
        herd.said("hub").contains("moved pack")
    })
    .await;
    let hub_said = herd.said("hub");
    let moved = hub_said
        .lines()
        .find(|line| line.contains("moved pack"))
        .unwrap_or_else(|| panic!("the hub said nothing about a transfer: {hub_said}"));
    assert!(
        moved.contains("Peer"),
        "the asker held none of this pack and had to dial the holder, so the path is Peer and \
         not SameHost (D10, E-12): {moved}"
    );
    assert!(
        moved.contains("1 object(s) fetched, 0 already held"),
        "and the figures must say the object really moved rather than being found at home: \
         {moved}"
    );
    // The byte figure, with its unit and the direction of its error: these
    // are **body bytes counted off the socket by the fetcher**, so they are a
    // lower bound on what crossed the loopback — the peer leg's header lines
    // are real bytes and are not in this count. What is asserted is only that
    // it is not nought, because nought is what both `SameHost` and a failed
    // transfer report and only a positive figure says a socket carried
    // something.
    let bytes: u64 = moved
        .split("by Peer: ")
        .nth(1)
        .and_then(|rest| rest.split(" byte(s)").next())
        .and_then(|figure| figure.trim().parse().ok())
        .unwrap_or_else(|| panic!("the transfer's byte figure is unreadable: {moved}"));
    assert!(
        bytes > 0,
        "a peer transfer that moved 0 body byte(s) off the socket moved nothing: {moved}"
    );
    // Printed as well as asserted, so that a reader with `--nocapture` has
    // the figure and its provenance rather than a report's recollection of
    // it. Captured by libtest otherwise, so it costs the suite nothing.
    eprintln!(
        "herd-test: {bytes} body byte(s) crossed the mesh, counted off the socket by the \
         fetcher — a lower bound on what left the holder, because the peer leg's header \
         lines are real bytes and are not in this figure"
    );

    // **The hub is not the byte path** (D10). The needle is the one string
    // that exists only inside the object, so this is a statement about the
    // pack's own bytes and not about its manifest, which rides the control
    // plane by design.
    let needle = only_inside_the_object(&commit);
    assert_eq!(
        where_the_hub_holds(&herd, &needle),
        None,
        "a byte of the pack passed through the hub, which is registry and authority and never \
         the byte path (D10)"
    );
    // The runner's own line, for the owner of the machine.
    assert!(
        herd.said("asker")
            .contains(&format!("is laid out for {PACK_KEY}")),
        "the asker must say which key it laid out: {}",
        herd.said("asker")
    );

    // Nothing here depended on a runner that died halfway.
    assert!(
        holder.alive(),
        "the holder must still be running: {}",
        herd.said("holder")
    );
    assert!(
        asker.alive(),
        "the asker must still be running: {}",
        herd.said("asker")
    );
    holder.ask_it_to_stop();
    asker.ask_it_to_stop();
}

/// **An outbound-only holder is refused by name, and no pack moves** (D10,
/// invariant 3).
///
/// Two things at once, and the second is why this test is not optional.
///
/// It is a **promise**: a runner given no `--mesh-listen` listens on nothing,
/// advertises nothing, and is therefore no use as a holder — and that is said
/// in a sentence naming the state, never left as a job that waits in silence
/// (D150). The bytes have to be *pushed* by such a holder instead, which is
/// the step after this one (ORCH-220).
///
/// And it is the **red** of the test above. The same fixture, the same two
/// labelled commands, one flag removed: the transfer that reports `Peer` with
/// bytes on it reports nothing at all here, and the job is handed back with a
/// sentence instead. So `Peer` is proven to be a measurement of the
/// advertisement and not of this harness — which is exactly the hole the
/// branch below this one fell into, where removing the `ExpectTransfer` send
/// left an end-to-end still passing.
#[tokio::test]
async fn an_outbound_only_holder_is_refused_by_name_and_nothing_moves() {
    let test = "an_outbound_only_holder_is_refused_by_name_and_nothing_moves";
    let Some((herd, checkout, _hub, mut holder, mut asker)) = two_machines(test, None).await else {
        return;
    };
    let commit = checkout.rev_parse(BRANCH);
    assert!(
        herd.said("holder").contains("outbound-only"),
        "a holder given no mesh flag must say it serves nothing: {}",
        herd.said("holder")
    );

    let forged = herd
        .herdctl_in(
            &checkout.root(),
            &["run", "--command", FORGE_HERE, "--commit", BRANCH],
        )
        .await;
    assert_eq!(
        status_code(&forged),
        0,
        "the producer must run: {}\n{}",
        stderr(&forged),
        herd.everything_said()
    );
    herd.wait_until("the hub says it holds the pack", || {
        herd.said("hub").contains(&format!("{commit}/{PACK_KEY}"))
    })
    .await;

    // Submitted and **not waited on**: a job whose pack cannot be got is
    // handed back and tried again, and `herdctl run` would quite correctly
    // sit there while that happened. What is read instead is the hub's
    // journal, which is where the sentence has to be.
    let submitted = herd
        .herdctl_in(
            &checkout.root(),
            &["submit", "--command", GRAFT_THERE, "--commit", BRANCH],
        )
        .await;
    assert_eq!(
        status_code(&submitted),
        0,
        "the submission itself is not the thing that fails: {}",
        stderr(&submitted)
    );
    herd.wait_until("the hub writes down that the pack cannot be got", || {
        herd.said("hub").contains("cannot get")
    })
    .await;
    let hub_said = herd.said("hub");
    let refusal = hub_said
        .lines()
        .find(|line| line.contains("cannot get"))
        .unwrap_or_else(|| panic!("the hub said nothing about it: {hub_said}"));
    assert!(
        refusal.contains("outbound-only"),
        "the refusal must name the state — every holder listens on nothing — and not merely \
         say the pack did not arrive (D150): {refusal}"
    );
    assert!(
        refusal.contains(&format!("{commit}/{PACK_KEY}")),
        "and name the key and the commit an operator would go looking for: {refusal}"
    );
    // And nothing moved: no transfer was reported at all, by either path.
    assert!(
        !hub_said.contains("moved pack"),
        "a transfer was reported for a pack no runner could serve: {hub_said}"
    );
    assert!(
        holder.alive() && asker.alive(),
        "both runners must still be running: {}",
        herd.everything_said()
    );
    holder.ask_it_to_stop();
    asker.ask_it_to_stop();
}
