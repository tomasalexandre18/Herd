//! E-48 against the real binaries: an unapproved runner claims nothing, one
//! operator action lets it in, and removing it takes the machine back out.
//!
//! The in-process tests cannot stand in for this one. What E-48 promises is
//! something a person does from a terminal — `herdctl runner approve wsl-pc`,
//! then a token pasted into a machine's configuration — and the property that
//! matters most is about a *file on the hub's disk*: the token an operator was
//! shown must not be readable back out of the hub's state. So everything here
//! is a real process, the approval goes through the real command, and the
//! state root is read as somebody with a shell on the server would read it.
//!
//! The harness — the processes, the temporary roots, the waits — is in
//! `real_herd`, shared with the other files that drive the real binaries.

mod real_herd;

use real_herd::{Herd, PATIENCE};

/// E-48, the whole of it: a runner nobody approved gets no job, and one
/// operator action is all it takes to fix that.
///
/// The three facts in one test, because they are one story and reading them
/// apart is how "it is refused" gets separated from "and then it works": the
/// machine is turned away with a sentence that says what to do, nothing can be
/// placed on it while that is true, and after one `herdctl runner approve` it
/// runs a real job to a real exit code.
#[tokio::test(flavor = "multi_thread")]
async fn an_unapproved_runner_gets_no_job_and_one_approval_lets_it_run() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;

    // ---- The machine nobody has let in.
    let mut turned_away = herd.start_runner_unapproved("wsl-pc", "unapproved");
    herd.wait_until("the unapproved runner is told why", || {
        herd.said("unapproved")
            .contains("is not approved on this hub")
    })
    .await;
    let said = herd.said("unapproved");
    assert!(
        said.contains("herdctl runner approve wsl-pc"),
        "the refusal has to say what to do about it: {said}"
    );
    assert!(
        said.contains("HERD_TOKEN"),
        "and where the token it mints then goes: {said}"
    );
    // Terminal, not a wait (D13): the runner gives up rather than dialling
    // every few seconds for an approval only a person can make.
    let ended = turned_away.gone().await;
    assert!(
        !ended.success(),
        "a runner the hub refused must not exit as though it had served"
    );

    // ---- And while that is true, nothing can be placed anywhere.
    let refused = herd
        .herdctl(&["submit", "--", "/bin/sh", "-c", "printf 'ran\\n'"])
        .await;
    assert!(
        !refused.status.success(),
        "a submission must not succeed when the only machine is unapproved"
    );
    let why = String::from_utf8_lossy(&refused.stderr).into_owned();
    // By the roster, which is the fact that makes this a refusal rather than
    // a wait: nobody is approved here, so nothing submitted could ever start
    // (D140). A machine that *is* approved and merely away queues the job.
    assert!(
        why.contains("0 runner(s) are approved on this state root"),
        "and the requester is told there is nowhere to run it, ever: {why}"
    );

    // ---- One operator action.
    let approval = herd.herdctl(&["runner", "approve", "wsl-pc"]).await;
    assert!(
        approval.status.success(),
        "the approval failed: {}",
        String::from_utf8_lossy(&approval.stderr)
    );
    let token = String::from_utf8_lossy(&approval.stdout).trim().to_owned();
    assert!(
        !token.is_empty(),
        "the token is on stdout alone, so a script can take it"
    );
    let explained = String::from_utf8_lossy(&approval.stderr).into_owned();
    assert!(
        explained.contains("printed once") && explained.contains("hash"),
        "the sentence has to say that this is the only time it is readable: \
         {explained}"
    );
    assert!(
        herd.herdctl(&["runner", "list"])
            .await
            .stdout
            .windows(6)
            .any(|window| window == b"wsl-pc"),
        "an approved runner is on the list: {}",
        String::from_utf8_lossy(&herd.herdctl(&["runner", "list"]).await.stdout)
    );

    // ---- And the same machine, now carrying that token, runs a job.
    //
    // `start_runner_called` asks the harness for the token of this name,
    // which is the one minted above: the harness approves a name once, so
    // this is the machine an operator has just let in and not a second one.
    let _runner = herd.start_runner_called("wsl-pc", "approved");
    herd.wait_for_runner().await;
    let ran = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'ran\\n'; exit 7"])
        .await;
    assert_eq!(
        ran.status.code(),
        Some(7),
        "the job's own exit code comes back: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        real_herd::job_output(&ran.stdout),
        "ran\n",
        "and its output is the job's, behind E-02's framing line"
    );
}

/// E-48's other half: removing a runner revokes its token, on the link it is
/// holding and on every link it makes afterwards.
///
/// A revocation that only took effect at the next dial would leave a machine
/// Herd no longer trusts holding a connection and claiming jobs for as long as
/// its process lived. So the test asserts the live connection: the runner is
/// told, in words, while it is connected.
#[tokio::test(flavor = "multi_thread")]
async fn a_removed_runner_can_no_longer_claim_and_is_told_why() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;
    let mut runner = herd.start_runner_called("wsl-pc", "runner");
    herd.wait_for_runner().await;

    // It is in the fleet: it runs a job before anything is revoked.
    let before = herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'in the fleet\\n'"])
        .await;
    assert!(
        before.status.success(),
        "an approved runner runs jobs: {}",
        String::from_utf8_lossy(&before.stderr)
    );

    // ---- Removed.
    let removal = herd.herdctl(&["runner", "remove", "wsl-pc"]).await;
    assert!(
        removal.status.success(),
        "the removal failed: {}",
        String::from_utf8_lossy(&removal.stderr)
    );
    let announced = String::from_utf8_lossy(&removal.stdout).into_owned();
    assert!(
        announced.contains("token is revoked") && announced.contains("has been told"),
        "the operator is told that the machine was connected and has been \
         told: {announced}"
    );

    // ---- The machine itself is told, on the link it was holding.
    herd.wait_until("the removed runner is told why", || {
        herd.said("runner")
            .contains("has been removed from this hub")
    })
    .await;
    let said = herd.said("runner");
    assert!(
        said.contains("claims nothing from here on"),
        "and told what that means for it: {said}"
    );
    assert!(
        said.contains("herdctl runner approve wsl-pc"),
        "and how it would come back: {said}"
    );
    let ended = runner.gone().await;
    assert!(
        !ended.success(),
        "a runner whose token was revoked must not exit as though it had served"
    );

    // ---- And nothing can be placed on it again.
    let after = herd
        .herdctl(&["submit", "--", "/bin/sh", "-c", "printf 'ran\\n'"])
        .await;
    assert!(
        !after.status.success(),
        "a submission must not succeed when the fleet has been emptied"
    );
    assert!(
        String::from_utf8_lossy(&after.stderr)
            .contains("0 runner(s) are approved on this state root"),
        "and the requester is told why: {}",
        String::from_utf8_lossy(&after.stderr)
    );
    // Not on the list any more either: a removed runner is gone, not hidden.
    let list = herd.herdctl(&["runner", "list"]).await;
    let listed = String::from_utf8_lossy(&list.stdout).into_owned();
    assert!(
        !listed.contains("wsl-pc"),
        "a removed runner is off the list: {listed}"
    );
    assert!(
        listed.contains("no runner is approved on this hub"),
        "and an empty roster says so rather than printing nothing: {listed}"
    );

    // ---- And dialling again with the revoked token is refused by name.
    let mut again = herd.start_runner_unapproved("wsl-pc", "again");
    herd.wait_until("the runner that comes back is refused by name", || {
        herd.said("again").contains("is not approved on this hub")
    })
    .await;
    let _ = again.gone().await;
}

/// The property the whole milestone rests on: the token an operator was shown
/// cannot be read back out of the hub's state (invariant 7, Q8).
///
/// Asserted by reading **every file under the state root**, as somebody with a
/// shell on the server — or a copy of the backup — would. A hash is there; the
/// token is not, in whole or in part.
#[tokio::test(flavor = "multi_thread")]
async fn a_token_is_never_readable_in_the_clear_from_the_hub_s_state() {
    let herd = Herd::new();
    let _hub = herd.start_hub("hub").await;

    let approval = herd.herdctl(&["runner", "approve", "wsl-pc"]).await;
    assert!(approval.status.success());
    let token = String::from_utf8_lossy(&approval.stdout).trim().to_owned();
    assert!(
        token.len() > 32,
        "a credential of substance: {}",
        token.len()
    );
    // The random half, so that a file carrying only the prefix every token
    // shares is not read as a file carrying this token.
    let secret = token
        .rsplit('-')
        .next()
        .expect("a token has a body")
        .to_owned();
    assert!(secret.len() >= 32, "the body is the secret: {secret}");

    // The runner is started too, so that the state root holds everything a
    // working hub writes and not only the roster.
    let _runner = herd.start_runner_called("wsl-pc", "runner");
    herd.wait_for_runner().await;
    assert!(herd
        .herdctl(&["run", "--", "/bin/sh", "-c", "printf 'ran\\n'"])
        .await
        .status
        .success());

    let mut looked_at = 0;
    for path in files_under(herd.state.path()) {
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("{} reads: {e}", path.display()));
        looked_at += 1;
        assert!(
            !contains(&bytes, token.as_bytes()),
            "{} carries the token in the clear",
            path.display()
        );
        assert!(
            !contains(&bytes, secret.as_bytes()),
            "{} carries the token's body in the clear",
            path.display()
        );
    }
    assert!(
        looked_at > 1,
        "the state root was read: {looked_at} file(s)"
    );

    // And what *is* there: the name, and a hash of the token under a salt.
    let roster: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(herd.state.path().join("runners.json")).expect("the roster reads"),
    )
    .expect("the roster is json");
    let entry = &roster["runners"][0];
    assert_eq!(entry["name"], "wsl-pc", "the roster names the machine");
    let hash = entry["token_sha256"]
        .as_str()
        .expect("an entry carries a hash");
    assert_eq!(hash.len(), 64, "sha-256 in hex is 64 characters: {hash}");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "and nothing but hex: {hash}"
    );
    assert_ne!(
        entry["salt"].as_str().expect("an entry carries a salt"),
        "",
        "an entry is salted"
    );
}

/// The one shared requester token (Q8): a hub started with one refuses a
/// requester that does not carry it, and serves the one that does.
#[tokio::test(flavor = "multi_thread")]
async fn a_requester_without_the_hub_s_token_is_refused_and_told_where_to_get_it() {
    let herd = Herd::new();
    // A file, never a value on the command line: a secret on argv is
    // readable out of `/proc` by every user of the host (invariant 7).
    let token_file = herd.logs.path().join("requester-token");
    std::fs::write(&token_file, b"a-shared-requester-token\n").expect("the token file is written");
    let _hub = herd
        .start_hub_with(
            "hub",
            &[
                "--requester-token-file",
                &token_file.display().to_string(),
                "--lost-after",
                "60",
            ],
        )
        .await;

    // Nothing without it.
    let refused = herd.herdctl(&["runner", "list"]).await;
    assert!(
        !refused.status.success(),
        "a requester with no token must not be served"
    );
    let why = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(
        why.contains("requires a requester token") && why.contains("HERD_TOKEN"),
        "and is told what is missing and how to give it: {why}"
    );

    // The wrong one is not the right one.
    let wrong = tokio::time::timeout(
        PATIENCE,
        tokio::process::Command::new(real_herd::binary("herdctl"))
            .arg("--hub")
            .arg(herd.url())
            .args(["runner", "list"])
            .env("HERD_TOKEN", "not-the-one")
            .output(),
    )
    .await
    .expect("herdctl comes back")
    .expect("herdctl runs");
    assert!(!wrong.status.success(), "the wrong token is refused");
    assert!(
        String::from_utf8_lossy(&wrong.stderr).contains("not the one this hub was started with"),
        "and said so by name: {}",
        String::from_utf8_lossy(&wrong.stderr)
    );

    // And the right one — trimmed, because the file was written with a
    // newline and a token that differed from itself by one byte would be a
    // refusal nobody could explain.
    let served = tokio::time::timeout(
        PATIENCE,
        tokio::process::Command::new(real_herd::binary("herdctl"))
            .arg("--hub")
            .arg(herd.url())
            .args(["runner", "list"])
            .env("HERD_TOKEN", "a-shared-requester-token")
            .output(),
    )
    .await
    .expect("herdctl comes back")
    .expect("herdctl runs");
    assert!(
        served.status.success(),
        "the token the hub was started with is served: {}",
        String::from_utf8_lossy(&served.stderr)
    );
}

/// Every file under a directory, recursively.
fn files_under(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut to_walk = vec![root.to_path_buf()];
    while let Some(directory) = to_walk.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|e| panic!("{} lists: {e}", directory.display()));
        for entry in entries {
            let entry = entry.expect("an entry reads");
            let path = entry.path();
            if path.is_dir() {
                to_walk.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

/// Whether `haystack` holds `needle` anywhere in it, as bytes: the state root
/// is read as bytes because a record may carry anything a job wrote (D18).
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}
