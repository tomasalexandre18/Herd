//! `herdctl`'s two halves meeting on one real repository (M10).
//!
//! [`herdctl::checkout`] and [`herdctl::declaration`] are each unit-tested on
//! their own, and that is not the thing M10 is judged on: a submission is one
//! commit read out of a checkout *and* one command read out of that same
//! checkout's declaration, and nothing until now has put the two together on a
//! repository that exists. The fixture in `fixture/mod.rs` is that repository —
//! real `git init`, a real bare remote, a real push — and these are the
//! assertions it makes possible:
//!
//! * a ref resolves to the hash the fixture's **own** git holds (E-01);
//! * that hash, a declared name and some arguments become exactly one
//!   [`herd_proto::Asked`], with `{commit}` and `{args}` filled and the
//!   runner's and the hub's tokens untouched (E-11, D107);
//! * a dirty tree and an unpushed commit are each refused, naming the file and
//!   the hash (E-08);
//! * **adding a third command changes nothing** — E-11's own acceptance
//!   sentence, and the test in this file that matters most.
//!
//! Every test builds its own fixture and asserts only on state that fixture
//! put there (D100, D101); none of them shares a repository with another, and
//! no hash is written into this file. Without `git` on the host there is no
//! repository to read, so each one skips loudly under its own name (D27)
//! rather than passing on nothing.
//!
//! The fixture's commands are invented (E-11): no name here is a consumer's.

use std::collections::BTreeMap;
use std::path::Path;

use herd_proto::{
    args_as_line, with_parallelism, LabelSet, Priority, IN_TOKEN, JOB_TOKEN, LINE_TOKEN, OUT_TOKEN,
    PARALLELISM_TOKEN,
};
use herdctl::declaration::{DeclaredPriority, Labels, Resolved, DECLARATION_PATH};

mod fixture;

use fixture::{
    refusal, Fixture, ASSEMBLE, BRANCH, ENV_ARGS, ENV_COMMIT, ENV_IN, ENV_OUT, ENV_REMOTE,
    LABELS_SCRIPT, PROBE, REMOTE, SKETCH, STEP_SCRIPT, TAG, TRACKED,
};

/// The arguments every test submits with: two words, one of which has a space
/// in it, because that is the one `args_as_line`'s quoting exists for.
fn args() -> Vec<String> {
    vec!["--only".to_owned(), "a b".to_owned()]
}

/// The baseline `probe` declares, as a [`LabelSet`].
fn toolchain() -> LabelSet {
    LabelSet::of("grove-toolchain").expect("a label of the fixture's own")
}

#[test]
fn a_ref_resolves_to_the_hash_the_fixture_s_own_git_holds() {
    let Some(fixture) = Fixture::new("a_ref_resolves_to_the_hash_the_fixture_s_own_git_holds")
    else {
        return;
    };
    // The independent reading: what this repository's git says, not a hash
    // this file carries.
    let held = fixture.rev_parse(BRANCH);
    assert_eq!(held.len(), 40, "{held}");
    assert!(
        held.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{held} is not forty lowercase hex"
    );

    let checkout = fixture.checkout();
    assert_eq!(
        std::fs::canonicalize(checkout.root()).expect("the checkout's root"),
        std::fs::canonicalize(fixture.root()).expect("the fixture's root"),
        "herdctl found a different work tree than the fixture built"
    );

    let branch = checkout.resolve(BRANCH).expect("the branch resolves");
    assert_eq!(branch.hash, held);
    assert_eq!(branch.named, BRANCH);

    // A tag on the same commit, peeled rather than refused.
    let tag = checkout.resolve(TAG).expect("the tag resolves");
    assert_eq!(tag.hash, held);
    assert_eq!(tag.named, TAG);

    // `HEAD`, which is what a caller passes when the requester named nothing.
    assert_eq!(checkout.resolve("HEAD").expect("HEAD resolves").hash, held);

    // A short hash: the fixture has one commit, so there is nothing for it to
    // be ambiguous with.
    let short = &held[..8];
    assert_eq!(
        checkout.resolve(short).expect("a short hash resolves").hash,
        held
    );

    // And the bare remote really holds that commit, which is the premise the
    // unpushed refusal is the other side of.
    assert_eq!(fixture.git_in("remote", &["rev-parse", BRANCH]), held);
}

#[test]
fn a_commit_and_a_declared_command_become_one_asked() {
    let Some(fixture) = Fixture::new("a_commit_and_a_declared_command_become_one_asked") else {
        return;
    };
    let held = fixture.rev_parse(BRANCH);
    let commit = fixture
        .checkout()
        .resolve(BRANCH)
        .expect("the branch resolves");
    assert_eq!(commit.hash, held);

    let commands = fixture.commands();
    assert_eq!(commands.path(), fixture.root().join(DECLARATION_PATH));
    let args = args();
    let resolved = commands
        .resolve(ASSEMBLE, &commit.hash, &args)
        .expect("the declaration resolves its own command");

    // The line, exactly: `{commit}` and `{args}` filled, `{j}` left for the
    // hub because the figure is a property of the machine it has yet to pick.
    assert_eq!(
        resolved.line,
        format!(
            "sh {STEP_SCRIPT} {ASSEMBLE} -j {PARALLELISM_TOKEN} -- {}",
            args_as_line(&args)
        )
    );

    let asked = resolved.declared().asked(
        resolved.command(),
        resolved.labels.at_submission().clone(),
        resolved.priority(Priority::Step),
    );

    // One shell line, never an argv Herd invented.
    assert_eq!(asked.command, vec![resolved.line.clone()]);

    let declaration = asked
        .declaration
        .as_ref()
        .expect("a declared command carries its declaration");
    assert_eq!(declaration.command, ASSEMBLE);
    assert_eq!(declaration.commit, held);
    assert_eq!(declaration.args, args);

    // The hooks: `{commit}` filled where `fetch` carries it, `{args}` filled
    // where `up` carries it, and nothing else touched.
    let executor = &declaration.executor;
    assert!(
        executor.fetch.contains(&held) && !executor.fetch.contains("{commit}"),
        "fetch is {:?}",
        executor.fetch
    );
    assert!(
        executor.up.contains(&args_as_line(&args)) && !executor.up.contains("{args}"),
        "up is {:?}",
        executor.up
    );
    assert!(
        executor.fetch.contains(&format!("${ENV_REMOTE}")),
        "fetch fetches from somewhere other than the declared remote: {:?}",
        executor.fetch
    );

    // The environment, value by value: two filled by herdctl, two left for the
    // runner, and the remote the fixture wrote in when it built itself.
    let env = &executor.env;
    assert_eq!(env.get(ENV_COMMIT).map(String::as_str), Some(held.as_str()));
    assert_eq!(
        env.get(ENV_ARGS).map(String::as_str),
        Some(args_as_line(&args).as_str())
    );
    assert_eq!(env.get(ENV_IN).map(String::as_str), Some(IN_TOKEN));
    assert_eq!(env.get(ENV_OUT).map(String::as_str), Some(OUT_TOKEN));
    assert_eq!(
        env.get(ENV_REMOTE).map(String::as_str),
        Some(fixture.remote().to_string_lossy().as_ref())
    );

    // The four tokens that are not the requester's, still here as text: the
    // runner's three and the one the `run` hook pastes the line into. `{j}` is
    // in the line rather than in a hook, so it is not in this list.
    assert_eq!(
        executor.tokens_left(),
        vec![JOB_TOKEN, IN_TOKEN, OUT_TOKEN, LINE_TOKEN],
        "the executor should still hold exactly the runner's tokens"
    );
    assert!(
        executor.run.contains(LINE_TOKEN),
        "run is {:?}",
        executor.run
    );

    // E-42's two keys, filled: a key still holding `{args}` would deduplicate
    // two submissions with different arguments into one.
    assert_eq!(
        resolved.dedup_key,
        Some(format!("{ASSEMBLE}-{held}-{}", args_as_line(&args)))
    );
    assert_eq!(
        resolved.coalesce, None,
        "{ASSEMBLE} declares no coalesce key"
    );
}

#[test]
fn the_two_commands_carry_the_figures_and_the_levels_they_declare() {
    let Some(fixture) =
        Fixture::new("the_two_commands_carry_the_figures_and_the_levels_they_declare")
    else {
        return;
    };
    let commit = fixture
        .checkout()
        .resolve(BRANCH)
        .expect("the branch resolves");
    let commands = fixture.commands();
    let args = args();

    let build = commands
        .resolve(ASSEMBLE, &commit.hash, &args)
        .expect("the build resolves");
    // A per-thread figure and no whole-job one: two answers to one question
    // would have been refused at parse.
    assert_eq!(build.build_thread_mib, Some(1792));
    assert_eq!(build.memory_mib, None);
    // A nought is a declaration and not an absence (E-31).
    assert_eq!(build.tmpfs_mib, Some(0));
    assert_eq!(build.timeout_s, Some(2700));
    assert_eq!(
        build.labels,
        Labels::Declared(
            LabelSet::new(["grove-toolchain", "linux"]).expect("the fixture's own labels")
        )
    );
    assert_eq!(
        build.labels.declared(),
        Some(&LabelSet::new(["grove-toolchain", "linux"]).expect("the fixture's own labels"))
    );
    assert_eq!(build.labels.reading(), None);
    // A fixed level wins over whatever the requester asked for.
    assert_eq!(build.priority, DeclaredPriority::Fixed(Priority::Producer));
    assert_eq!(build.priority(Priority::Merger), Priority::Producer);
    // Neither of M11's two fields: the pack chain is the appended pair, and
    // the two commands that are always here declare no pack. This file
    // declared `leaves` at two **files** until the layout step made the field
    // travel — a `leaves` path is a pack, and a pack is a directory holding
    // `manifest.tsv`, `pack.meta` and `objects/` (M11, E-17) — and a
    // declaration Herd cannot honour is worse than none.
    assert!(build.needs.is_empty(), "{:?}", build.needs);
    assert!(build.leaves.is_empty(), "{:?}", build.leaves);
    assert_eq!(build.schedule, None);

    let test = commands
        .resolve(PROBE, &commit.hash, &args)
        .expect("the test run resolves");
    assert_eq!(test.memory_mib, Some(4096));
    assert_eq!(test.build_thread_mib, None);
    assert_eq!(test.tmpfs_mib, None);
    assert_eq!(test.timeout_s, Some(7200));
    // A reading, so the declared labels are the baseline the reading job is
    // placed against and the job's own labels are not known yet (D74).
    assert_eq!(
        test.labels,
        Labels::Read {
            line: format!("sh {LABELS_SCRIPT} {} {}", commit.hash, args_as_line(&args)),
            baseline: toolchain(),
        }
    );
    assert_eq!(
        test.labels.declared(),
        None,
        "a reading's labels are not known until it has run"
    );
    assert_eq!(test.labels.at_submission(), &toolchain());
    // No fixed level: a merger's run of this command goes in front.
    assert_eq!(test.priority, DeclaredPriority::Requester);
    assert_eq!(test.priority(Priority::Merger), Priority::Merger);
    assert_eq!(test.priority(Priority::Nightly), Priority::Nightly);
    // The reading command declares no pack either, and for the sharper half
    // of the same reason: a command with `needs` waits at the dispatch until
    // one is registered for its commit (D150), so the six tests here that run
    // this command would have waited for ever rather than failed.
    assert!(test.needs.is_empty(), "{:?}", test.needs);
    assert!(test.leaves.is_empty(), "{:?}", test.leaves);
    assert_eq!(test.coalesce, Some(format!("{PROBE}-{}", commit.hash)));
    assert_eq!(test.dedup_key, None, "{PROBE} declares no dedup key");

    // **The pack chain, and that its two halves line up**, which is the pair
    // M11 reads. Read off the declaration the addition pushed, because a pack
    // is registered for a `(commit, key)` pair and the chain arrives on a
    // commit of its own.
    let hash = fixture.add_the_pack_chain();
    let commands = fixture.commands();
    let producer = commands
        .resolve(fixture::FORGE, &hash, &args)
        .expect("the pack producer resolves");
    let consumer = commands
        .resolve(fixture::GRAFT, &hash, &args)
        .expect("the pack consumer resolves");
    assert_eq!(
        producer.leaves,
        BTreeMap::from([(fixture::PACK_KEY.to_owned(), fixture::PACK_AT.to_owned())])
    );
    assert!(producer.needs.is_empty(), "{:?}", producer.needs);
    assert_eq!(consumer.needs, vec![fixture::PACK_KEY.to_owned()]);
    assert!(consumer.leaves.is_empty(), "{:?}", consumer.leaves);
    assert!(
        consumer
            .needs
            .iter()
            .all(|key| producer.leaves.contains_key(key)),
        "the fixture's pack chain does not line up: {:?} against {:?}",
        consumer.needs,
        producer.leaves
    );
}

#[test]
fn a_dirty_tree_is_refused_naming_the_file_it_changed() {
    let Some(fixture) = Fixture::new("a_dirty_tree_is_refused_naming_the_file_it_changed") else {
        return;
    };
    let checkout = fixture.checkout();
    // The baseline, established rather than assumed: clean and pushed, this
    // branch resolves. Whatever the refusal below is about, it is about the
    // change and not about the repository.
    let clean = checkout.resolve(BRANCH).expect("a clean tree resolves");
    assert_eq!(clean.hash, fixture.rev_parse(BRANCH));

    let changed = fixture.dirty(TRACKED);

    let why = refusal(checkout.resolve(BRANCH));
    assert!(
        why.contains(&changed),
        "the refusal does not name the file that changed: {why}"
    );
    assert!(
        why.contains("1 change that is in no commit"),
        "the refusal does not say what is wrong: {why}"
    );
    assert!(
        why.contains("nothing gets past this refusal"),
        "the refusal offers a way past it: {why}"
    );

    // Resolution comes first, so an unknown name reads as an unknown name even
    // here: a person who mistyped a branch must not be told about their tree.
    let unknown = "no-such-ref-in-this-fixture";
    let why = refusal(checkout.resolve(unknown));
    assert!(
        why.contains(unknown) && why.contains("is not a ref or a commit"),
        "an unknown name was not reported as one: {why}"
    );
    assert!(
        !why.contains(&changed),
        "an unknown name was reported as a dirty tree: {why}"
    );

    // And the declaration is readable throughout: a dirty tree is checkout's
    // refusal and says nothing about which commands the file declares.
    assert_eq!(fixture.commands().names(), vec![ASSEMBLE, PROBE]);
}

#[test]
fn an_unpushed_commit_is_refused_naming_the_hash() {
    let Some(fixture) = Fixture::new("an_unpushed_commit_is_refused_naming_the_hash") else {
        return;
    };
    let checkout = fixture.checkout();
    let pushed = checkout
        .resolve(BRANCH)
        .expect("the pushed commit resolves");

    let unpushed = fixture.commit_unpushed("src/only-here.txt");
    assert_ne!(unpushed, pushed.hash);
    // The branch now points at it, and the tree is clean: one refusal to find.
    assert_eq!(fixture.rev_parse(BRANCH), unpushed);

    let why = refusal(checkout.resolve(BRANCH));
    assert!(
        why.contains(&unpushed),
        "the refusal does not name the hash: {why}"
    );
    assert!(
        why.contains("is on no remote-tracking ref"),
        "the refusal does not say what is wrong: {why}"
    );
    assert!(
        why.contains(REMOTE),
        "the refusal does not name the remote it looked on: {why}"
    );

    // The commit that *was* pushed still resolves, so the refusal is about one
    // commit and not about a repository that stopped working.
    assert_eq!(
        checkout
            .resolve(&pushed.hash)
            .expect("the pushed commit still resolves")
            .hash,
        pushed.hash
    );
}

#[test]
fn a_command_the_declaration_does_not_hold_is_refused_listing_the_ones_it_does() {
    let Some(fixture) =
        Fixture::new("a_command_the_declaration_does_not_hold_is_refused_listing_the_ones_it_does")
    else {
        return;
    };
    let commit = fixture
        .checkout()
        .resolve(BRANCH)
        .expect("the branch resolves");
    let commands = fixture.commands();
    // What this checkout declares, established by reading it.
    assert_eq!(commands.names(), vec![ASSEMBLE, PROBE]);

    let mistyped = "assembl";
    let why = refusal(commands.resolve(mistyped, &commit.hash, &args()));
    assert!(
        why.contains(mistyped),
        "the refusal does not quote what was asked for: {why}"
    );
    assert!(
        why.contains(ASSEMBLE) && why.contains(PROBE),
        "the refusal does not list the commands this checkout declares: {why}"
    );
    assert!(
        why.contains(&fixture.root().join(DECLARATION_PATH).display().to_string()),
        "the refusal does not say which file it is about: {why}"
    );
}

/// E-11's own acceptance sentence, and the most load-bearing test in this
/// file: a consumer adds a command, and nothing on Herd's side moves.
///
/// "Nothing" is made exact here. The two commands that were already declared
/// are resolved through the same calls, for the same commit and the same
/// arguments, before and after — and the two [`Resolved`] values, and the two
/// [`herd_proto::Asked`] values built from them, must compare equal. Not "look
/// similar": equal, field by field, which is what deriving `PartialEq` on them
/// is for.
#[test]
fn a_third_command_changes_nothing_about_the_first_two() {
    let Some(fixture) = Fixture::new("a_third_command_changes_nothing_about_the_first_two") else {
        return;
    };
    let checkout = fixture.checkout();
    let commit = checkout.resolve(BRANCH).expect("the branch resolves");
    let args = args();

    let before = fixture.commands();
    assert_eq!(before.names(), vec![ASSEMBLE, PROBE]);
    // Established, so that adding it proves something: the third command is
    // not declared yet, and asking for it is refused.
    let why = refusal(before.resolve(SKETCH, &commit.hash, &args));
    assert!(
        why.contains(SKETCH) && why.contains(ASSEMBLE) && why.contains(PROBE),
        "the third command was somehow already there: {why}"
    );
    let build_before = before
        .resolve(ASSEMBLE, &commit.hash, &args)
        .expect("the build resolves before");
    let test_before = before
        .resolve(PROBE, &commit.hash, &args)
        .expect("the test run resolves before");
    let asked_before = asked_for(&build_before);
    let executor_before = before.executor().clone();

    let third = fixture.add_third_command();
    assert_ne!(third, commit.hash, "the third command was really committed");
    // Pushed, so a runner could fetch the commit that declares it.
    assert_eq!(
        checkout
            .resolve(BRANCH)
            .expect("the commit declaring the third command resolves")
            .hash,
        third
    );

    let after = fixture.commands();
    assert_eq!(after.names(), vec![ASSEMBLE, PROBE, SKETCH]);
    assert_eq!(
        after.executor(),
        &executor_before,
        "the executor changed when a command was added"
    );

    // The same calls, the same commit, the same arguments — the same answers.
    assert_eq!(
        after
            .resolve(ASSEMBLE, &commit.hash, &args)
            .expect("the build resolves after"),
        build_before,
        "adding a third command changed what the first one resolves to"
    );
    assert_eq!(
        after
            .resolve(PROBE, &commit.hash, &args)
            .expect("the test run resolves after"),
        test_before,
        "adding a third command changed what the second one resolves to"
    );
    assert_eq!(
        asked_for(
            &after
                .resolve(ASSEMBLE, &commit.hash, &args)
                .expect("the build resolves after")
        ),
        asked_before,
        "adding a third command changed the submission the first one makes"
    );

    // And the third resolves, on the commit that declares it, with no Rust
    // written for it.
    let sketch = after
        .resolve(SKETCH, &third, &args)
        .expect("the third command resolves");
    assert_eq!(
        sketch.line,
        format!("sh {STEP_SCRIPT} {SKETCH} -- {}", args_as_line(&args))
    );
    assert_eq!(sketch.declaration.command, SKETCH);
    assert_eq!(sketch.declaration.commit, third);
    assert_eq!(sketch.labels, Labels::Declared(toolchain()));
    assert_eq!(sketch.priority, DeclaredPriority::Requester);
    assert_eq!(sketch.timeout_s, Some(600));
    assert!(sketch.declaration.executor.fetch.contains(&third));
}

/// The fixture's hooks and the command line it declares are shell, and they
/// run: the four steps in order, the commit fetched from the fixture's own
/// bare remote, and a step log a reader can see.
///
/// This is here because the end-to-end test M10 still owes will run these
/// hooks through a real runner, and a fixture whose hooks were a plausible
/// string would make that test fail for the fixture's reasons rather than
/// Herd's. What a runner and the hub fill is filled here the way they fill it:
/// `{job}`, `{in}`, `{out}` and `{line}` through [`herd_proto::Executor::filled`],
/// and `{j}` through [`with_parallelism`].
#[test]
fn the_four_hooks_the_fixture_declares_really_run() {
    let Some(fixture) = Fixture::new("the_four_hooks_the_fixture_declares_really_run") else {
        return;
    };
    let commit = fixture
        .checkout()
        .resolve(BRANCH)
        .expect("the branch resolves");
    let args = args();
    let resolved = fixture
        .commands()
        .resolve(ASSEMBLE, &commit.hash, &args)
        .expect("the build resolves");

    // What the hub fills: the `-j` it derived for the machine it picked.
    let threads = 4;
    let filled = with_parallelism(&resolved.command(), &args, threads);
    assert_eq!(filled.len(), 1, "a declared line is one shell line");
    let line = &filled[0];
    assert!(
        !line.contains(PARALLELISM_TOKEN) && line.contains("-j 4"),
        "the line still carries {PARALLELISM_TOKEN}: {line}"
    );

    // What a runner fills: its own three directories, and the line.
    let where_it_runs = tempfile::TempDir::new().expect("a directory for the job");
    let consumed = where_it_runs.path().join("in");
    let left = where_it_runs.path().join("out");
    std::fs::create_dir_all(&consumed).expect("the job's input directory");
    std::fs::create_dir_all(&left).expect("the job's output directory");
    let job = "grove-job-under-test";
    let executor = resolved
        .declaration
        .executor
        .filled(JOB_TOKEN, job)
        .filled(IN_TOKEN, &consumed.to_string_lossy())
        .filled(OUT_TOKEN, &left.to_string_lossy())
        .filled(LINE_TOKEN, line);
    assert!(
        executor.tokens_left().is_empty(),
        "the declaration names a token nobody fills: {:?}",
        executor.tokens_left()
    );

    let mut printed = String::new();
    for step in herd_proto::Step::ALL {
        let hook = executor.hook(step);
        let said = fixture.shell(where_it_runs.path(), &executor.env, hook);
        assert!(
            said.status.success(),
            "the {step} hook failed: {hook}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&said.stdout),
            String::from_utf8_lossy(&said.stderr)
        );
        printed.push_str(&String::from_utf8_lossy(&said.stdout));
    }

    // The step log the hooks wrote, in the workspace `up` made.
    let log = std::fs::read_to_string(where_it_runs.path().join(job).join("steps.txt"))
        .expect("the hooks wrote a step log");
    let steps: Vec<&str> = log.lines().collect();
    assert_eq!(
        steps,
        [
            format!("up {job}"),
            format!("fetch {}", commit.hash),
            "run".to_owned(),
            format!("{ASSEMBLE} -j 4 -- --only a b"),
            format!("down {job}"),
        ]
        .iter()
        .map(String::as_str)
        .collect::<Vec<&str>>(),
        "the four steps did not run in order, or the line did not run: {log}"
    );

    // `fetch` really fetched: the commit is checked out in the workspace, from
    // the fixture's own bare remote and from nowhere else.
    let workspace = where_it_runs.path().join(job);
    assert_eq!(
        git_in(&workspace, &["rev-parse", "HEAD"]),
        commit.hash,
        "the workspace is not at the commit the submission named"
    );
    assert!(
        workspace.join(STEP_SCRIPT).is_file(),
        "the fetched tree does not hold the tool the declared line runs"
    );

    // And the job's own output says what ran, which is what a requester reads.
    assert!(
        printed.contains(&format!("grove: workspace {job} args: --only a b")),
        "the output does not carry the arguments: {printed}"
    );
    assert!(
        printed.contains(&format!("grove: at {}", commit.hash)),
        "the output does not carry the commit: {printed}"
    );
    assert!(
        printed.contains(&format!("grove {ASSEMBLE}: -j 4 -- --only a b")),
        "the output does not carry the declared line's own run: {printed}"
    );
}

/// The adversarial half of the hub's `{j}`, through the **real declaration**:
/// an argument that says `{j}` is three characters a requester typed, and the
/// declared `{j}` beside it is still a thread count (D107, D139, D140).
///
/// Here rather than only in `herd_proto` because this is the whole chain the
/// defect lived in: the fixture's own declared line, resolved with real
/// arguments quoted into it before the job is minted, then the hub's one pass
/// over it. The failure direction is conservative — a declared token left
/// literal, never a requester's text turned into a figure.
#[test]
fn a_parallelism_token_in_an_argument_is_not_filled_by_the_hub_s_pass() {
    let Some(fixture) =
        Fixture::new("a_parallelism_token_in_an_argument_is_not_filled_by_the_hub_s_pass")
    else {
        return;
    };
    let commit = fixture
        .checkout()
        .resolve(BRANCH)
        .expect("the branch resolves");
    let args = vec!["--only".to_owned(), PARALLELISM_TOKEN.to_owned()];
    let resolved = fixture
        .commands()
        .resolve(ASSEMBLE, &commit.hash, &args)
        .expect("the build resolves");
    let line = resolved.command();
    assert_eq!(
        herd_proto::parallelism_tokens(&line, &args),
        1,
        "the declaration wrote one {PARALLELISM_TOKEN}; the argument's is not Herd's to count \
         — two would end the job by name: {line:?}"
    );
    let filled = with_parallelism(&line, &args, 4);
    assert_eq!(filled.len(), 1, "a declared line is one shell line");
    assert!(
        filled[0].contains("-j 4"),
        "the declared token is the figure: {}",
        filled[0]
    );
    assert!(
        filled[0].contains(&format!("'{PARALLELISM_TOKEN}'")),
        "and the requester's own word is untouched: {}",
        filled[0]
    );
}

/// The `labels_from` line the fixture declares is shell too, and what it
/// prints is an answer Herd reads (D74).
///
/// Two things, and the second is the one worth having: the line the
/// declaration holds, filled, really runs in the checkout; and what it prints
/// is what [`herd_proto::Requirements::read`] reads, with the figures the
/// fixture's own script prints. Where that answer then goes — a reading job,
/// the cache, the record — is the end-to-end suite's, not this one's.
#[test]
fn the_reading_line_the_fixture_declares_really_runs() {
    let Some(fixture) = Fixture::new("the_reading_line_the_fixture_declares_really_runs") else {
        return;
    };
    let commit = fixture
        .checkout()
        .resolve(BRANCH)
        .expect("the branch resolves");
    let args = args();
    let resolved = fixture
        .commands()
        .resolve(PROBE, &commit.hash, &args)
        .expect("the test run resolves");
    let reading = resolved
        .labels
        .reading()
        .expect("this command declares a reading");
    assert!(
        reading.contains(&commit.hash) && reading.contains(&args_as_line(&args)),
        "the reading line was not filled: {reading}"
    );

    let said = fixture.shell(&fixture.root(), &resolved.declaration.executor.env, reading);
    assert!(
        said.status.success(),
        "the reading line failed: {reading}\n{}",
        String::from_utf8_lossy(&said.stderr)
    );
    // And what it printed is an answer, read the way the hub reads one.
    let answer = herd_proto::Requirements::read(&said.stdout).unwrap_or_else(|why| {
        panic!(
            "the reading printed something that is not an answer: {why}\n{}",
            String::from_utf8_lossy(&said.stdout)
        )
    });
    assert_eq!(
        answer,
        herd_proto::Requirements {
            labels: LabelSet::new(fixture::READ_LABELS).expect("the fixture prints labels"),
            memory_mib: fixture::READ_MEMORY_MIB,
            tmpfs_mib: fixture::READ_TMPFS_MIB,
            scenarios: fixture::READ_SCENARIOS,
        },
        "the reading answered something else: {}",
        String::from_utf8_lossy(&said.stdout)
    );

    // And the submission carries the reading rather than the labels, so that
    // the hub knows there is one to run (D74).
    let asked = asked_for(&resolved);
    assert_eq!(
        asked.labels_from,
        Some(herd_proto::LabelsFrom {
            line: reading.to_owned(),
            baseline: toolchain(),
        }),
        "a submission of a command with a labels_from carries it, filled"
    );
    assert_eq!(
        asked.labels,
        toolchain(),
        "and its labels are the baseline the reading is placed against, not the job's own"
    );
}

/// What a submission carries for one resolved command, built the one way
/// `herdctl` builds it.
fn asked_for(resolved: &Resolved) -> herd_proto::Asked {
    resolved.declared().asked(
        resolved.command(),
        resolved.labels.at_submission().clone(),
        resolved.priority(Priority::Step),
    )
}

/// One git read in a directory these tests made, for the independent reading of
/// what a hook left behind.
fn git_in(at: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {} failed in {}: {}",
        args.join(" "),
        at.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
