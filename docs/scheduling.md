# A pass that submits itself when a branch has moved

What Herd ships of a scheduled pass, what the consumer writes, and the one
thing E-41 asks for that cannot be given the way it is written.

The command is `herdctl if-moved`. It submits a declared command **if a
named ref has moved since the last run**, and says so on stderr when it has
not. `herdctl if-moved --help` is the reference; this is the shape and the
reasoning.

## What we ship, and what is yours

We ship **one predicate and one submission**. We do not ship a scheduler,
and this is not a step towards one: a workflow language is what invariant 1
refuses, and a timer is something every host this will ever run on already
has.

| Who | What |
| --- | --- |
| Herd | the predicate (has this ref moved?), the state file, the lock that keeps two runs from both submitting, and the submission itself |
| the consumer | **when** the question gets asked: a `systemd` timer, a cron line, whatever already runs on that machine — and the declaration the command comes out of |

That split is the same one E-42 already has: the push hook is written and
owned in the consumer's own tree, and Herd ships the `herdctl` call it
makes. A timer is one unit file; a scheduler inside the hub would be a
repository, a working copy and a git dependency on the hub, which D106
settled against and D207 reaffirmed for this feature by name.

## Why it is on the requester's side

`docs/plan.md`'s M14 says the hub keeps a bare repository and decides from
it. **That is wrong on the record** (D207, 2026-10-29) and the plan is what
is wrong, not D106: the hub holds no repository and runs no git. It accepts
a job, matches it against declared capabilities and priority, and nothing
about a scheduled pass changes that. A hub that cloned somebody's
repository to decide when to build would also need a credential for it.

So the ref is read where the repository already is — in `herdctl`, by the
same module that reads the commit any submission carries (`herdctl::moved`,
`herdctl::checkout`).

## The state file

`--state <FILE>` is "the last run", in your own tree. It is small JSON,
carries its own `state_format_version`, and names the ref and the command
it answers for:

```json
{
  "state_format_version": 1,
  "ref": "refs/remotes/origin/main",
  "command": "nightly",
  "commit": "8f1c2d…",
  "recorded_at": "2026-10-29T03:00:01Z",
  "job_id": "j-4812"
}
```

**The ref is read once**, and that one sha is three things: what the
predicate compared, what the job builds, and what the file records. A ref
that moves between the question and the submission does not produce a job
at one commit recorded as another.

**Every way the file can be wrong is a refusal naming the file**: missing,
unreadable, written at another format version, kept for another ref, kept
for another command. None of them is read as "the ref moved". The cheapest
wrong answer here would be to treat an empty directory as a move, and it
costs a whole machine for the length of a nightly.

So the first run is a deliberate act:

```sh
herdctl if-moved --init --command nightly \
  --ref refs/remotes/origin/main --state /var/lib/chatons/nightly.json
```

`--init` records where the ref stands now and submits nothing; the first
pass runs the next time it moves. A file that is already there is refused
rather than replaced, because replacing it is how last night gets
forgotten. If you want a pass *now*, submit one by hand — that is
`herdctl run --command nightly --commit <ref>`, and it is a different act.

One file per ref and per command. The file carries both and refuses a run
that asks about either one differently, which is what keeps two passes
sharing a path from reading as a move every time the other one lands.

## Two timers that overlap

A nightly that fires twice because two timers overlapped is the failure
this guards, so it is enforced rather than assumed. The whole of
read-decide-submit-record happens while an exclusive lock is held on
`<FILE>.lock`, and a second run submits nothing and says so. The lock is
`flock`-shaped: the kernel drops it when the process ends, however it ends,
so a killed run leaves nothing stale and no timeout has to be guessed.

What the second run says is deliberately narrow — another run holds the
pass — and it claims nothing about what that run decided. That run holds
the file and records what it does.

## The three lines, and the exit code

All three go to **stderr**, and a wrapper reads the prefix, never the
wording:

```
herdctl: moved: ...        a job was submitted; its id is on stdout
herdctl: not-moved: ...    the ref is where the last run left it
herdctl: another-run: ...  a run of this pass is already going
```

All three **exit 0**. A night with nothing to build is not a failure, and a
timer unit that went red on one would be red most mornings.

A refusal exits 125 with the reason on stderr, as everything else in
`herdctl` does, and `herdctl: no-job: …` says plainly that nothing was
submitted (`herdctl --help`).

**It submits and leaves**, as `herdctl submit` does, and never follows the
job: a timer unit that held a process for the five hours of a nightly
would be a timer holding a process for five hours, and the job runs and is
recorded whether anyone is watching or not. The id on stdout is what
`herdctl wait <id>` picks up later, and from that moment the job is the
hub's to report on — which is exactly why the only skip with no event is
the one where no job was made.

## The one thing E-41 asks for that this cannot give

**E-41 asks for a skipped pass to be reported as an event. A skip that
submitted nothing is this command's own line and not an event, and no hub
work will change that.** The hub emits events about jobs it knows; a run
that submits nothing leaves it with nothing to emit, because it never heard
of the job. The report is `herdctl: not-moved:` on the timer's own stderr,
which is where `journalctl -u` already looks.

The skip that **is** an event is the other one, and E-41 wants both: a pass
that *was* submitted, with `--deadline 05:00+02:00`, that no machine had
started by then. The hub cancels that job on its deadline and reports it
with the reason naming what it was waiting for (E-09, D40) — a real job,
a real record, a real event. That half needs nothing new.

Two skips, then, and they are not the same skip:

| What happened | Where it is reported |
| --- | --- |
| the ref had not moved, or a run was already going | `herdctl`'s own stderr line, exit 0. No job exists, so no event |
| a pass was submitted and nothing started it by its deadline | the hub: the job ends `cancelled (deadline)`, exit 125, and the event is the hub's (E-09) |

## The timer, as an example and not as a deliverable

This is the consumer's to write, keep and change. It is here so that the
shape is not guessed, and `05:00` is E-41's own bound:

```ini
[Unit]
Description=Ask Herd for a nightly pass if main moved

[Service]
Type=oneshot
WorkingDirectory=/srv/chatons
ExecStartPre=/usr/bin/git fetch --quiet origin main
ExecStart=/usr/bin/herdctl if-moved \
  --command nightly \
  --ref refs/remotes/origin/main \
  --state /var/lib/chatons/herd-nightly.json \
  --deadline 05:00
```

Three things about it are not decoration:

- **the fetch is theirs and comes first.** `herdctl` reads refs and never
  moves them: a ref nobody fetched has not moved, and would be reported as
  not moved every night, for ever, truthfully.
- **`--ref` is a remote-tracking ref** in the ordinary case, because that
  is the ref a fetch moves. A local branch nothing pushed is refused at
  submission anyway: a runner fetches the commit from the consumer's own
  origin (E-08).
- **`--deadline` is what carries E-41's 05:00**, and it is an ordinary
  submission's deadline (E-09). The declaration's own `schedule.at` and
  `schedule.deadline` are **not** read by this command: they are times of
  day, and turning one into an instant needs a day and a time zone the file
  does not name. That is the timer's business, and the timer is where the
  day already is. `schedule.if_moved` **is** read, as the default for
  `--ref`, because a ref's name is complete in itself.

## Two sharp edges, written down rather than discovered

- **A dirty checkout refuses the submission**, even though the commit being
  submitted is a ref's tip and not the working tree. `if-moved` submits
  through the same door as `herdctl submit --command`, and that door refuses
  a tree with tracked changes in no commit (E-08, no flag past it). Nothing
  is recorded when it refuses, so the next run tries again — but a pass
  whose checkout is left dirty will not run. Keep the checkout the timer
  uses clean, or give the timer one of its own.
- **If the state file cannot be written after a job was submitted**, the
  job exists and the file does not say so, so the next run will submit the
  same commit again. The window is microseconds wide and the sentence is
  loud, naming the job and the file. It is this way round on purpose:
  recording before submitting would turn a hub that is down into a commit
  that is silently never built, and a pass that never runs and says nothing
  is worse than one that runs twice.
