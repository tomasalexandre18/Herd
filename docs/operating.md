# Operating a Herd hub and its runners

This is for you: the one person who runs a hub and a runner in docker
compose on a machine nobody in this repository has ever logged into and
nobody ever will. Herd is shipped, not operated. That means every fact
about your instance reaches us as a report you send, and it means this
page has to answer the questions you would otherwise have to ask —
before you have to ask them.

Every claim here was either read out of the code it describes, or
measured. The ones nobody has checked are named as unchecked, in the
sentence that makes them. Where a figure is a default in the source, the
file is named so you can read it yourself out of the tree the release
was built from.

`deploy/release-notice.md` is the page for the moment you download a
release: the files, the two version numbers in the release body, the
runner's four requirements, the exit codes. `docs/joining.md` walks one
machine into the fleet. This page is the one for afterwards — for
reading what your hub holds, changing what bounds it, and for the two or
three mornings a year when something refuses.

## Four words, and the two you use differently

Everything below uses exactly these:

| Word | What it is |
| --- | --- |
| **hub** | the service. One instance, in docker compose, with one state root on one volume |
| **runner** | a client that offers a machine's capacity and executes jobs. It dials out to the hub |
| **requester** | whoever submits a job — a person, a script, an AI agent — through `herdctl` |
| **job** | one unit of requested work, with requirements, a priority, an exit code and logs |

Your own issues say **client** where this says *runner*, and **agent**
where this says *requester*. That is the whole of the mapping; nothing
else differs. The words in the table are the ones in every message, every
log line and every `--help` text, so when you report something, using
them saves a round trip.

## Reading what the state root holds

The hub's state root is one directory: `/var/lib/herd` inside the
container, a docker volume outside it (`herd_herd-state`, if you took
`deploy/docker-compose.yml` as it is). It holds one directory per job and
a handful of small files beside them. `docs/architecture.md` §10 maps
every one of them.

Two subcommands of the hub binary read it and nothing else:

* **`herd-hub report`** — what this root holds. Three figures: the
  file-level deduplication ratio over the records, per-job output size
  **by decile** and its maximum, and what the records claim against what
  the directories measure. It prints the counts under every ratio, never
  a bare one.
* **`herd-hub retention-preview`** — what record retention *would*
  remove and what it would cost. **It removes nothing**: there is no
  deletion path in it and no flag that could be given one.

Both are safe against a hub that is serving, and that is a property of
the code rather than a promise. Neither one builds the store, so neither
takes the store's lock; above all neither takes the `flock(2)` on
`hub.lock`, which a running hub holds for its whole life. They open
files read-only, one at a time, create nothing and write nothing.
`report` reads records and the *lengths* of the files under each job's
`files/`, `files.part/` and `files.gone/` — no output byte and no log
byte (`herd-hub/src/report.rs`, module documentation).

What you give up for that, and the report says so itself at the top of
its own output: the figures are not one consistent instant. A hub
writing this root while the walk is in progress can land a record after
the walk passed its directory. Each record is published by `rename`, so
each one is read whole or not at all; only *which* version of it you
read is in doubt.

### The recommended way: a disposable container, read-only, no network

The method in this section is **yours**. You used it, it is better than
what this project had documented, and you agreed we could reproduce it:
run the **release's own static `herd-hub` binary** in a throwaway
container with the state volume mounted **read-only** and **no network**.

```sh
# 1. Verify the binary you are about to trust.
sha256sum --ignore-missing -c SHA256SUMS

# 2. Read the root. Nothing is started, nothing is written, nothing dials.
docker run --rm --network none \
  --entrypoint /herd-hub \
  -v "$PWD/herd-hub-0.4.0-x86_64-unknown-linux-musl:/herd-hub:ro" \
  -v herd_herd-state:/var/lib/herd:ro \
  herd-hub:0.3.7 report /var/lib/herd
```

The base image there is the hub image you already have loaded, chosen for
exactly that reason: with `--network none` nothing can be pulled, so the
image has to be on the host already. Any x86_64 Linux image does — the
binary is statically linked against musl and depends on no libc, no
dynamic loader and no Nix store — but the one you already have needs no
download. `--entrypoint` is what replaces that image's own hub with the
binary you mounted.

Five things that makes true, and the first is why it is the recommended
way rather than one of several:

1. **The serving hub is never touched.** No `exec` into a live
   container, so no command of yours can land inside the process that is
   holding your fleet's state.
2. **The volume is read-only**, so the method cannot write even if the
   binary tried to. It does not try — but a mount flag is checkable and
   an intention is not.
3. **No hub is started.** `report` prints and exits; it never reaches
   the code that binds a port or takes `hub.lock`. With `--network none`
   it could not serve anything even if it did.
4. **It works when your serving image is older than the subcommand.**
   `report` and `retention-preview` are in v0.4.0's hub and **not** in
   v0.3.7's, so if your hub is at 0.3.7 then
   `docker compose exec hub herd-hub report` cannot work and this can.
   That is the case the method was invented for.
5. **It tells you what is in the root before any hub opens it.**
   `report` **names each record it cannot read, counts it, and goes
   on**, so a damaged `job.json` is something you read off a command you
   chose to run rather than something you meet at a restart. On every
   released version up to and including v0.4.1 it is also the *only*
   thing that can read such a root at all, because one unparseable
   `job.json` makes those hubs refuse the whole state root and exit
   before binding a port — see *Two known defects* below.

If your hub's image is at a version whose binary already carries these
subcommands, then `docker compose exec hub herd-hub report` also works
and needs no download at all. It is still the second choice: it runs
inside the container that is serving.

### The retention preview, and its two refusals

`retention-preview` takes **a span per priority class, four required
arguments, every one with its unit and none with a default**. A shipped
binary stating a retention policy would be a number nobody could later
change, so this one states none. `0` means **never expires**, and the
output says it in those words.

```sh
docker run --rm --network none \
  --entrypoint /herd-hub \
  -v "$PWD/herd-hub-0.4.0-x86_64-unknown-linux-musl:/herd-hub:ro" \
  -v herd_herd-state:/var/lib/herd:ro \
  herd-hub:0.3.7 retention-preview /var/lib/herd \
  --merger 90d --step 14d --nightly 14d --producer 14d
```

Per class it prints how many records are past that span, what they
weigh, the oldest and the newest of them, and the clock reading every
figure was judged against.

It **refuses**, prints no per-class figures at all and exits non-zero,
in two cases. Neither is a threshold anybody tuned:

* **the clock reads earlier than this root's own newest record.** The
  hub wrote that record with its own clock, so the clock has gone
  backwards since; every record would read as younger than it is. Wrong
  in the safe direction, and still meaningless, so it does not run.
* **the pass would leave no record behind.** This is the forward-clock
  refusal, and it is the only shape that catches one: a clock stepped
  forward by a year is indistinguishable from a hub that has been idle
  for a year, and nothing on the disk tells them apart. What *is*
  visible is the consequence they share — a pass that takes everything —
  and since nothing in Herd can undo a removal, that consequence is what
  gets refused.

A refusal exits non-zero deliberately: the figures you would have acted
on were not printed, so the exit status must not read as "here they
are".

### What `report` cannot tell you

It is arithmetic over records and file lengths. It does not tell you
whether a job passed, what a log said, or what any requester is waiting
for. It names what it could not read rather than skipping it, so a
record this build cannot parse appears in a count and, up to ten of
them, by path — which means a low `records read` is a fact about the
root and not a quiet zero.

## The status document and the text screen: not yet shipped

There is **no machine-readable status document and no text screen in any
released build**. They are E-35, planned in M13 (`docs/plan.md` M13;
`docs/coverage.md` E-35, state `planned`). A task in this project is
building them as this page is written; until a release carries them,
nothing on your host has them, and this section will be rewritten
against what ships rather than against what was intended.

What answers the same questions today, all of it shipped:

| Question | What answers it now |
| --- | --- |
| which machines are approved, which are here, which build each runs, and which capabilities each one is **not** handing out, with the reason | `herdctl runner list` |
| where each runner's switch is, and the reason its owner gave | `herdctl switch` |
| why a queued job is still queued | `herdctl wait <id>`, which restates it on a cadence the hub sets |
| what the hub has decided and what the sweeper did | the hub's own stdout and stderr: `docker compose logs -f hub` |
| what one job ended as, for a script | `herdctl run --verdict-to <file>` or `herdctl wait --verdict-to <file>` |
| what the state root holds | `herd-hub report`, above |

`herdctl runner list` is the line to read first when a job is not
starting. A capability that is suspended is not a refusal — jobs needing
it **wait** — but a capability suspended because its probe is broken
waits for ever, and the fleet looks healthy from every other angle.

## The two budgets and the two retention spans

Four figures bound your disk. Two are budgets, in bytes; two are spans,
in time. They are set on different processes and they bound different
things, and the one mistake worth avoiding is reading any of them as the
size of the state root.

| Figure | Where | Default | What it bounds |
| --- | --- | --- | --- |
| `--output-budget` (`HERD_OUTPUT_BUDGET`) | the hub | `200GB`, decimal (`herd-hub/src/job.rs`) | the **output files only**, summed over whole, verified collections in every job's `files/` across the whole root. Not records, not logs |
| `--job-disk-budget` (`HERD_JOB_DISK_BUDGET`) | each runner | 20 GiB (`herd-runner/src/confine.rs`) | how much **one job's workspace** may hold on that machine before the runner stops the job |
| `--output-floor` (`HERD_OUTPUT_FLOOR`) | the hub | 7 days (`herd-hub/src/job.rs`) | how long a collection is promised before the sweeper may take it |
| record retention | the hub | **not enforced by any shipped build** | how long a job's *record* is kept. Decided: merger 90 days, every other class 14 (D201) |

A third runner figure sits next to these and is worth knowing so you do
not confuse it with a budget: `--job-output-cap` bounds how much one job
may hand **back**, 1 GiB by default
(`herd-runner/src/confine.rs:82`). Over it no file comes back at all and
the record says so with both figures — never a truncated file, because a
cut-down `results.json` that still parses reads like the truth. **Known
documentation defect:** `herd-runner --help` still describes this option
as "32 MiB by default, which is also the most this can be", which was
true of an older build and is not true of v0.4.0. The constant is the
authority; the help text is wrong and is ours to fix.

**The budget counts only whole collections.** A collection still
crossing the link is real disk and deliberately outside the budget,
because evicting a transfer in flight would destroy the thing the hub is
being given. So a root can measure more than its budget while transfers
are in progress, and that is not a fault.

### What happens at the floor

The floor is **a floor, not a lifetime**, and the two are opposite
things. Nothing is removed for merely being older than seven days — a
collection outlives the floor by as long as the budget allows. What the
floor does is keep a *fresh* collection safe while there is anything
older to take instead.

The sweeper fires on an **arrival**, never on a clock: bytes landing add
to a running total and a sweep happens when that total crosses the
budget. Under budget, a sweep reads no records at all. Over budget it
reads a record per byte-holding job and takes them in this order:
outside the floor first, then the jobs that passed, then the ones
already fetched, then the oldest. A collection still arriving is never
taken.

When honouring the floor would mean going over the budget, **the budget
wins** — the disk is what enforces anything — and the hub says so on the
day, on stderr, naming both figures:

```
herd-hub: 3 of those collection(s) were still inside this hub's 7 days
retention floor. 7 days is what this hub promises unless the fleet
produces more than the 200GB budget in less than that — it just did, and
the budget is what the disk enforces. Raise --output-budget, or expect
this again.
```

A floor that broke quietly would be worse than no floor, because the
next thing that happens is a full disk nobody saw coming. The counts in
that block and in the next one are illustrative; the sentences are the
hub's own, and the two sizes in each are your budget and your floor.

Each evicted job's own record and `events.jsonl` keep the reason for
ever — `removed 2026-10-30 under the 200GB output budget`, with the age
and the floor added when the collection was inside it. **The record
keeps every path, size and digest for ever: only the bytes are ever
removed.** That is the tombstone, and it has a section of its own below.

There is one more line, said **once per episode** and not once per
sweep, for the case where the hub is over budget and may remove nothing:

```
herd-hub: 214GB of job output is held against a 200GB budget and nothing
more may be removed: 19GB of it belongs to collections that are still
arriving or whose records will not read, and a transfer in flight is
never taken. The volume will stay over budget until those finish.
```

The flag clears when a sweep comes back inside the budget, so a second
episode is a second line.

### Changing one, and what it costs

* **Both hub figures are read at startup**, and opening the state root
  **sweeps**. So a change takes effect at the next hub start, before any
  job runs: edit the environment, `docker compose up -d`, and the first
  thing the new hub does is enforce the new number. Lowering the budget
  therefore evicts on that start, not later — read the root *before* you
  lower it, because nothing in Herd can bring bytes back.
* **Stopping the hub is cheap and loses nothing.** The hub answers
  `SIGTERM` with a bounded stop path: it stops accepting connections,
  starts no further dispatch pass, waits up to **five seconds** for the
  pass in flight, and exits (D184). `deploy/docker-compose.yml` allows
  20 s, so the stop is never cut short. A runner holds a verdict the hub
  has not acknowledged and delivers it on the next start.
* **The runner figures are read at its startup too**, so changing one is
  a unit restart. `systemctl stop herd-runner` drains and exits inside
  five seconds (invariant 2).
* **Record retention costs nothing to change today, because no shipped
  build enforces it.** When the engine lands it will ship **active by
  default** (D201) — which is worth saying plainly, because it means a
  shipped Herd will remove records without being asked to. The guard it
  ships behind is `retention-preview`, its two refusals, and `0` meaning
  never.

### What is changing, and is not shipped

Retention's **basis** is moving. Until now the spans above were a
calendar: a record of class X is kept for N days. Alexa has just decided
that the basis becomes **free space** instead. That decision is newer
than this page's own evidence: it is not recorded as a numbered decision
in `docs/decisions.md` at the commit this page was written on, no code
implements it, and nothing in any release behaves that way.

So: what is **shipped** is the output budget and the output floor,
exactly as the table above gives them. What is **decided and unbuilt**
is the 90/14 class map and that it ships active (D201). What is
**changing under that** is the move from a calendar to free space, with
no design, no figure and no release yet. Set your hub's two figures on
what is shipped; expect the record half to arrive differently from the
way the plan documents describe it, and expect a release notice to say
so when it does.

## Approving a runner, revoking one, updating one

Three verbs and no more. There is no "rotate", because approving again
*is* the rotation, and no "show me the token", because the hub keeps only
a hash and has nothing to show.

```sh
herdctl runner approve wsl-pc     # approve, and print its token once
herdctl runner remove   wsl-pc    # revoke it
herdctl runner list               # who is approved, who is connected
```

### Approve

The token goes to **stdout, alone and once**; the sentence explaining it
goes to stderr. So `token=$(herdctl runner approve wsl-pc)` is all a
script needs. The hub stores a 16-byte salt and `sha256(salt, token)` in
`runners.json` and never the token, so a leaked state root leaks no
credential.

Then put it where only that machine's runner can read it — the recipe is
in `deploy/release-notice.md` and `docs/joining.md`; it is
`/etc/herd/runner-token.env`, mode 0400, owned by the runner's user, read
through `EnvironmentFile=` and never on a command line.

**Approving a name that is already approved mints a new token and
revokes the old one.** That is both how you replace a token that has
leaked and how you recover when a token is lost between the hub and the
machine. The runner carrying the old one is then refused by name and
claims nothing.

An **empty roster refuses every runner**, which is the right default and
not a misconfiguration to work around.

### Revoke

`herdctl runner remove <name>` revokes that runner's token. If it is
connected it is told so on the link it is holding, and claims nothing
from that moment:

```
runner wsl-pc has been removed from this hub and its token is revoked, so
it claims nothing more
```

**A job it is already executing is left to finish, or to be lost the
ordinary way.** A removal is not a way to kill a build — `herdctl cancel
<id>` is, and it reaches the runner executing the job wherever it is.

If what you want is the machine back rather than the machine out, use the
switch instead: `herd-runner switch off --reason "…"` on the machine
itself, or `herdctl switch <runner> off --reason "…"` through the hub.
Both write the same file under the runner's data root, so the position
outlives the command and a reboot, and `off` lets what is running finish.
`herdctl switch` reports success only once the runner has written the
position down and fsynced it, within ten seconds; anything short of that
exits 2 and says what nobody has confirmed.

### Update a runner

**There is no self-update in any shipped build.** A runner updating
itself on the hub's word is M15d, planned and unbuilt
(`docs/plan.md` M15d). So updating a runner is: put the new binary on the
machine, and restart it.

```sh
sha256sum --ignore-missing -c SHA256SUMS
sudo install -m 0755 herd-runner-<version>-x86_64-unknown-linux-musl \
  /usr/local/bin/herd-runner
herd-runner --version
sudo systemctl restart herd-runner
```

Three things to know before you do it on a machine that is in the fleet:

* **Within a major protocol version the two sides may differ by patch.**
  A runner says so at startup and the hub serves the connection anyway.
  A *protocol* move is not that: a hub refuses a runner announcing a
  different `PROTOCOL_VERSION` **by name**, naming both numbers, and the
  refusal is terminal. A protocol move is a fleet-wide, synchronised
  step — the procedure is in `deploy/release-notice.md`.
* **The stop drains.** `systemctl stop herd-runner` claims nothing new,
  lets what is running finish and exits inside five seconds; a verdict
  the hub has not acknowledged by then is written under the data root and
  delivered on the next start. Stopping loses nothing.
* **v0.4.0 is the exception, and it deletes bytes.** See *Two known
  defects* below before you move any runner to it.

After the restart, read the runner's own startup report. It is the only
thing that knows what your host actually granted, and it changes when
the host changes, not only when the binary does.

## Reading a refusal: "not here, ever" against "waiting for room"

The two look alike in a list of queued jobs and call for opposite
actions. One means *change the submission*; the other means *wait, or add
capacity*. The hub never confuses them, and the distinction is in what it
says, not in an exit code.

### A job that waits

A job whose labels nobody offers **today** waits. That is deliberate: a
machine that offers them may dial in tomorrow. You will see a sentence
like `no connected runner can run it: …` or `no runner is connected to
this hub`, restated to whoever is following the job on a cadence the hub
sets (`--wait-restatement`, 60 s by default). A job can wait for ever
this way, and the only bound on one no connected machine could ever fit
is the deadline its requester gave it (`--deadline`).

Three other waits read differently and are worth recognising:

* **a suspended capability.** `herdctl runner list` prints it with its
  reason, and so does the job's own waiting sentence. A probe that is
  broken suspends a capability and the job waits for ever.
* **stickiness.** A job goes to the most preferred machine that can run
  it and **waits for that machine while it is connected** rather than
  falling back to a free lesser one. `--stickiness-limit` bounds that
  wait, and the figure is in the waiting sentence.
* **a queued requirement reading.** One runner runs at most two at once
  (`--readings-at-once`); beyond that a reading **queues** and is named
  in the sentence of every job waiting behind it. It is never refused.

### A job that is refused for ever

`impossible` is the one terminal answer the placement step gives, and it
is deliberately narrow: **what no fleet can fix.** Two labels that
exclude each other. A derived `-j` with no figure to derive it from. A
job whose requirements are read on a machine and whose reading did not
answer. `herdctl` exits **125** and prints:

```
no runner could ever run this job, so it was not queued for one: <reason>
```

The reason names the labels or the figure, because the only thing to do
about one of these is change the submission.

### And the third one, which is neither

`refused_as_full` is the opposite of impossible: **every machine it was
offered to could have run it and none had room.** Nothing failed, nothing
was lost, no process was killed, and every runner that refused it was on
the line and healthy. What ran out is the hub's own bound on how many
such refusals one job may collect. Also exit 125:

```
no machine had room for this job: it was handed straight back 8 time(s)
of the 8 this hub allows, by server (2 of 2), wsl-pc (1 of 1). Nothing
ran and nothing failed. <reason>
```

Eight is the shipped bound (`herd-hub/src/server.rs`); the machine names
and their figures there are illustrative and the wording is the tool's
own. The answer to this ending is capacity — or finding out why the
hub's count and a runner's disagree.

### Telling them apart from a script

Do not parse the sentence. `herdctl run --verdict-to <file>` and
`herdctl wait --verdict-to <file>` write the job's end as JSON, and
`cause` is one word: `exited`, `signalled`, `stopped_by_cap`,
`could_not_start`, `setup_failed`, `cancelled`, `lost`, `impossible`, or
`no_verdict` when Herd never got one. `refused_as_full` carries its
figures as fields beside it — how many refusals, the bound, and every
machine that said it was full with the figure it gave. Every field is
always present; one with nothing in it is `null`. The first field is
`verdict_format_version`; check it before anything else.

The three endings above all wear **125**, and that is on purpose: 125 is
"Herd has no verdict", never a failing build. A script that reads 125 as
"the tests failed" is the one mistake that cannot be survived.

## Reading a tombstone answer

A job's bytes can be gone while the job's record is still there. Ask for
the files anyway: you get an **answer**, not an empty directory.

The hub sends every file it has and then one frame saying what it could
not send. For each absent path it carries the path, **how big it was**
off the manifest, why it is gone, how many bytes of it the hub holds, and
one field that decides everything — whether the hub is **still expecting
those bytes**. `herdctl` prints the collection's own sentence first and
then the names:

```
j0000001234: 12 file(s), 338 MiB, removed 2026-10-30 under the 200GB
output budget — every path, size and digest is still on this job's
record, and they are named below
files/screenshots/001.png (1048576 bytes on this job's record) is not on
the hub: removed 2026-10-30 under the 200GB output budget
…
```

Illustrative figures again; the two sentences are `herdctl`'s own.

The one line exists because printing only the per-file lines would make
you count twelve of them to learn one fact. The per-file lines stay,
because **a short directory must never read as a whole one**.

**"Not yet" and "gone" are different, and they must never be confused.**
A job's verdict goes first and its bytes follow, so a requester asking
for a collection the instant its job ended is asking for something
genuinely in flight. Told "gone" it would report a lost result for
evidence that was seconds away. So: while the hub is still expecting the
bytes, `herdctl` says nothing and asks again — and the figure for how
much the hub holds is what tells you waiting is working. A collection
still crossing and one that will never arrive look identical in a list of
names; the difference is whether that figure moves.

### Why the manifest outlives the bytes

Because the alternative is silence, and the next thing that happens after
evidence disappears without a reason is somebody concluding that Herd
lost it.

A record keeps every path, every size and every sha256 for as long as the
record exists. Only the bytes are ever removed. So months later the
question *what happened to that job's screenshots* has an answer on the
disk — which files there were, how big, and the sentence saying they were
removed on a named day under a named budget. `herd-hub report` counts
that case as its own: jobs whose record says the bytes are gone, and the
bytes their manifests still name. Non-zero there is not a defect.

## When a version refusal names two figures

Read this one before you need it.

Three version numbers travel with a Herd build, and the release body
prints the two that matter on lines a script can match:

| Number | What it governs | Today |
| --- | --- | --- |
| `PROTOCOL_VERSION` | whether two builds can talk | **2** |
| `RECORD_VERSION` | the shape of every `job.json` in the root | **1** |
| `LAYOUT_VERSION` | the shape of the root itself | **1** |

`PROTOCOL_VERSION` is a fleet problem and `deploy/release-notice.md`
covers it. The other two are a **disk** problem, and this is the section
about them.

**There is no migration path anywhere.** A hub at a higher record or
layout version does not convert an existing state root. There is no
offline migration tool, no `--migrate`, nothing planned to add one. But
the two numbers are refused in two different ways, and that difference
is the whole of what you have to plan for.

**A layout number that differs refuses the root.** It is read once, out
of `store.json`, before the hub binds anything, and the hub **does not
start** rather than guess:

```
the state root /var/lib/herd has layout version 1, and this hub writes
version 2
```

**A record number that differs does not refuse the root — since
2026-10-29.** It is read per record, and a record carrying a number this
build does not read takes the same path as a damaged one: it is
**quarantined**, named with its path, counted, left untouched and kept
out of the queue, and the hub comes up (D218).

```
/var/lib/herd/jobs/j0000001234/job.json has record version 1, and this
hub reads version 2
```

So a build whose `RECORD_VERSION` differs from your root's while its
`LAYOUT_VERSION` matches **starts, and serves an empty queue**: every
job in the root is quarantined, each one named on the hub's log with its
path, and the total is on the line that says the hub is listening. That
is loud, and it is not a refusal — nothing stops the hub and nothing
stops the deployment. **Do not rely on the hub to stop you here.** Read
the two releases' numbers yourself.

The layout refusal cuts both ways. **It is the rollback too:** if the
layout numbers differ between the release you are on and the release you
are moving to, then neither the upgrade nor the rollback across them can
read your state root. Crossing **either** number means starting on a
**fresh** root, leaving every queued job and every uncollected verdict
behind in the old one. Compare both numbers before you move an image in
either direction; equal numbers mean a move is a `docker load` and
nothing more.

### The instruction, in order

When a hub refuses your root with two numbers in the sentence, the order
matters more than the steps:

1. **Take the reading off the old root first.** Before you move
   anything, before you start anything, run `herd-hub report` on the old
   root — the disposable-container recipe above, with **the old
   release's** binary, the one whose numbers match that root. Once the
   root is aside you will not get another chance, and once you are on a
   build whose numbers differ you cannot read it at all. Save the
   output.
2. **Then move the root aside — do not convert it, and do not delete
   it.** Stop the hub (`docker compose stop hub`; it exits inside five
   seconds and loses nothing). Leave the old volume exactly as it is and
   give the new hub a **new, empty** volume, by changing the volume this
   deployment names. Nothing is destroyed that way, and the old root
   stays readable by the old binary for as long as you keep it.
3. **Expect every runner to be refused once, by name, and that is
   correct.** A fresh root mints a new identity, and a runner holding a
   spooled verdict for the old root's `j0000000001` is holding state for
   a hub that no longer exists — the same job ids mean different jobs
   there. The hub says so to the runner and prints it:

   ```
   this hub serves the state root <new>, and this peer announced <old>:
   whatever it is holding — a spooled verdict, above all — belongs to
   another hub's state, where the same job ids mean different jobs.
   Nothing of it is believed here. Deal with it and dial again
   ```

   The runner sets such a spool aside under a `.foreign-state-root` name
   rather than deleting it, names it with its size at every start, and
   leaves the disk to you.
4. **Re-approve nothing you do not have to.** The roster lives in the
   root, so a fresh root has an empty one and refuses every runner. Each
   machine needs one `herdctl runner approve <name>` and a new token.

### What this means for you, concretely, today

Nothing shipped moves `RECORD_VERSION` or `LAYOUT_VERSION`. All three
numbers are 2, 1 and 1 in v0.4.0, and the next ordinary release is
constrained to ship at record 1 and layout 1 so that your existing root
stays readable (D199). The release that moves them is **v1.0.0**, and
moving them was taken as a decision with its price understood: a fleet
restarting from zero. When that release comes, the four steps above are
the procedure, and step 1 is the one nobody can do for you.

## One limitation, stated rather than left silent

**The requester token is shared.** It is one token, in one file, carried
by every requester. There is no per-requester identity and no priority
ceiling attached to one, so **any requester holding that token may ask
for the highest priority level** — `merger` — and may `herdctl raise`
another job to it. That is a real limitation of what ships, not a
misconfiguration you can close, and it is not being designed around here.

The hub without a token file at all is wider still, and it says so as it
starts:

```
herd-hub: NO requester token is set, so anything that can reach this hub
may submit, cancel and approve runners. That is only safe on loopback
behind the reverse proxy; set --requester-token-file
(HERD_REQUESTER_TOKEN_FILE) before this hub is reachable from anywhere
else
```

Runners are not covered by that token: each carries one of its own,
minted by `herdctl runner approve` and kept hashed in the root. An
unapproved runner is refused whatever the requester token says.

## Two known defects, with their mitigations

Both were found on 2026-10-29. Both are defects with mitigations, not
properties of the design, and both are named here because you are the
person who will meet them. **Both are fixed on `main` and carried in no
release**, so each section says what its fix changes and which versions
are affected. Until a release carries them, the mitigations are what you
have.

### A torn `job.json` refuses the whole state root (v0.4.1 and below)

**What happens, on the hub you are running today.** One `job.json` that
does not parse — a record torn by a crash or a power cut mid-write —
makes the hub refuse the **entire** state root at startup. The error
leaves `main` before `Hub::bind_with` is reached, so **the hub exits
before binding its port**, and a container with `restart:
unless-stopped` restarts straight back into the same wall. One torn
record takes every healthy job with it; the crash-point enumeration had
a case losing two healthy jobs to one torn third. The index cannot route
around it: the hub reads every record the index omits and sets the index
aside on an unreadable one, so recovery reads it again and the refusal is
unconditional.

Verified at **v0.4.1**, which is the newest release and the newest build
that does this: `herd-hub/src/store.rs` turned an unparseable `job.json`
into an error while a *rejected label set* three lines on returned a
quarantined job instead; `recover` propagated it, `open_with` propagated
that, and `main` propagated that — 76 lines before the port is bound.

**Why it is wrong and not merely unfortunate.** Every other degraded
path in that store degrades loudly and carries on: the readings cache,
the index, `log.seq`, a torn `events.jsonl` tail, and a record whose
labels today's rules reject. It was the only place in that store with no
degraded mode, which is why it was fixed. And on a released build
**nobody but you can repair it**, because the repair is "delete a file
inside the consumer's volume" and nobody in this project touches your
machine.

**Mitigation, and it is the order of operations above.** `herd-hub
report` is **not** affected the same way: it pushes an unreadable record
onto its unread list and continues, so it counts what it cannot read
instead of refusing everything. So read the root before you start a hub
on it — which is exactly why the disposable-container recipe comes first
on this page, and why it matters most on a root you have not opened for
weeks. If a hub does refuse this way, `report` will name the path, and
the path is what you remove.

One limit on that mitigation, stated because it is a real gap and not a
quibble: `report` and the hub do not parse a record the same way.
`report` reads only the two fields it needs and lets serde ignore the
rest, so a record missing a field the hub requires is counted in
`records read` by `report` and **quarantined** by the hub. A clean
`report` is therefore evidence about damaged *bytes*, not a promise that
every record the hub will open is one it can use.

**What the fix changes.** On `main`, a record the hub cannot read is
**quarantined** instead of refusing the root: the hub binds its port,
every healthy job on the root runs, and the damaged record is named with
its path at every start, counted on the line that says the hub is
listening, written to `index/state.json`, kept out of the queue, never
dispatched and **never rewritten** (D218). A requester asking about that
job is told it is quarantined and why — never "no such job". What still
refuses a whole state root is a root the hub cannot *enumerate*: `jobs/`
unlistable, `store.json` unreadable or of a layout version this build
does not write, the lock unheld.

| Version | A `job.json` it cannot read | The hub binds its port |
| --- | --- | --- |
| v0.3.7 and below | refuses the state root | no |
| v0.4.0 | refuses the state root | no |
| **v0.4.1** | **refuses the state root** | **no** |
| `main` after the fix | quarantines that one record | yes |

No version number moves for it: no frame and no record shape changed.
**It is on `main` and in no release.** Until a release carries it, run
`report` on the root before you start a hub on it.

**A `RECORD_VERSION` mismatch, because this page promised the
opposite.** Until 2026-10-29 this section said that a `RECORD_VERSION`
mismatch would deliberately **stay a hard error**, on the reasoning that
damage and a wrong binary are different failures. **That is not what was
built.** On `main` a record carrying a version this build does not read
takes the same quarantine path as a torn one, per record: the hub starts,
that record is set aside and named, and the rest of the root is served.
The reasoning behind the old promise still holds for a whole root a
different Herd wrote, and `LAYOUT_VERSION` in `store.json` is where it
now lives — that number still refuses the root outright. The per-record
arm is what changed, and *When a version refusal names two figures*
above says what it means for an upgrade.

### Upgrading a runner to v0.4.0 deletes output spools

**What happens.** v0.4.0's runner deletes, at startup, the output spools
left under `<data root>/outputs/` by a collection that was interrupted —
the spools its *own* startup sweep exists to re-adopt and offer to the
hub again. Both halves ran on the same startup, twenty-eight lines
apart, over the same directory: the reaper cleared it and then the
re-adoption read it. The feature was dead on arrival in the release that
introduced it.

**Which versions.** Measured against the tags in this repository:

| Version | Reaper clears `outputs/` | Re-adopts a spool | Affected |
| --- | --- | --- | --- |
| v0.3.6 | no | no | no |
| v0.3.7 | no | no | no |
| **v0.4.0** | **yes** | yes | **yes** |
| `main` after the fix | no | yes | no |

So **v0.4.0 is the only released version that does this**, and
v0.3.6 and v0.3.7 do not. If you are holding a runner below v0.4.0
because of it, that is the right call: upgrading would delete the bytes
you are waiting for, and nothing can bring them back.

**What the fix changes.** The startup reaper owns **processes,
workspaces and cgroups, and nothing else**; `outputs/` becomes the
sweep's alone (D211). Removing a spool goes back to being the hub's
sentence — the hub says whether anyone is still owed the bytes — and when
the hub stays silent the bytes are **set aside** under a name that says
so (`.unclaimed`, `.foreign-state-root`), named with their size at every
start, and the disk is left to you:

```
3 spool(s) under /var/lib/herd-runner/outputs have already been set aside
under a .unclaimed or .foreign-state-root name: no hub will ever ask for
them, this runner does not offer them, and removing them is yours to do
```

No version number moves for it: it is a bug fix inside `herd-runner`,
with no frame and no record shape changed. **It is on `main` and in no
release.** Wait for the release that carries it before you move a runner
to v0.4.0 or above, and read that release's notice to confirm the fix is
named in it.

**The lesson, since it explains why a green test did not catch this:**
both halves had a passing test and **neither test ran the startup
sequence**. One drove a fake runner over a raw socket and never called
the runner's own startup; the other re-adopted a spool it had placed
itself. Two green tests, one dead feature. The fix's test calls the
runner's own two entry points and re-implements neither.

## What nobody has checked

Named here rather than left to be discovered.

* **`wss://` has never been dialled for real.** The TLS path is
  exercised by tests and no human has made one real dial through a
  reverse proxy into a hub. There is deliberately no `--insecure` and no
  trust-this-CA option: if the certificate in front of the hub is not
  signed by a public CA, the dial fails and nothing on a command line
  changes that. The two paths proven today are `ws://127.0.0.1:8787` on
  the hub's own host and plain `ws://` over a private mesh the machine's
  owner already runs.
* **No eviction has ever run in production.** The order the sweeper
  takes — outside the floor, then passed, then fetched, then oldest — is
  tested and has not been observed under a real crossing on a real
  fleet. The first time it fires on your volume will be the first time it
  has fired anywhere.
* **No record retention has ever run anywhere**, because the engine does
  not exist. What exists is the preview.
* **Nothing in this project has measured your disk.** Every figure this
  project holds about your instance is a report somebody relayed, dated,
  and marked unverified in `docs/disk-policy.md` §1.2. Nobody here has
  read your job directories and nobody will.
* **Herd does not return files to a requester yet** beyond what fits the
  cap above; direct runner-to-runner transfer is M11's unbuilt half. A
  job that writes into `{out}` produces a file that is collected into the
  record, and the directories a declared command gets are made and empty
  until that half lands. If you need something out of a job today, print
  it: the log comes back.

## Where to go next

| You want | Read |
| --- | --- |
| what a release's files are, the two numbers in its body, the runner's four requirements, the exit codes | `deploy/release-notice.md` |
| one machine into the fleet, end to end | `docs/joining.md` |
| the first start of the hub under compose, and the two ownership failures it hits | `deploy/README.md` |
| what is on disk, field by field | `docs/architecture.md` §10 |
| why the budget is 200 GB and the floor seven days | `docs/disk-policy.md` |
| what a job may and may not do on a runner's host | `docs/runner-confinement.md` |
