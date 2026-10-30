# A delegated user scope blinds the preflight's cgroup check

- **Date:** 2026-10-30, measured while probing what it would take to make
  the twelve cgroup-delegation assertions on this workstation.
- **Observed:** inside an uncapped `systemd-run --user --scope -p
  Delegate=yes`, every level the preflight walks reads no ceiling.
  `user.slice`, `user-1002.slice`, `user@1002.service`, `app.slice` and
  the scope itself all read `memory.max=max` and `memory.high=max`, and
  `/sys/fs/cgroup` itself has **neither file**. `bench-preflight.sh`
  check 4 then prints *"slice room: nothing from … up to the root of
  /sys/fs/cgroup carries a memory.high or a memory.max, reading
  unavailable"* and **clears**: that branch says a sentence and never
  calls `refuse`. Confirmed in the script at `scripts/bench-preflight.sh`
  lines 367-371, which needs no scope to read.
- **Why it matters more than a blind check:** the clause was added on
  2026-10-29 *because* five agents each lost a whole suite run to the
  18 GiB `memory.high` on `chatons.slice`. A delegated user scope is
  outside `chatons.slice`, so it escapes the one ceiling that bounds
  everything else this project does on this host — and the check that
  would have noticed is the check the scope silenced. **The method that
  unlocks twelve assertions disables check 4.** In the finder's words,
  kept because the sentence is the lesson: *"on this host, 'nothing
  encloses me' now means 'I am in a user scope', not 'I am on a bare
  machine'."*
- **Expected now:** any scope or unit installed to get cgroup
  delegation **carries its own memory ceiling**. Measured: adding
  `-p MemoryMax=12G` restored a real reading — *"12281 MiB under
  memory.max on the scope, floor 8192"* — and check 4 cleared on its
  merits rather than on an absence. It is safe for the tests: the
  assertion about a tighter enclosing ceiling asserts its own 512 MiB
  child, and the baseline already passes under an 18 GiB enclosure, so
  an enclosing ceiling is this host's normal condition and not a new
  variable. The recipe in `docs/runner-confinement.md` carries the flag.
- **Verify:** no scope needed for the dangerous half. Read the branch:
  `sed -n '367,371p' scripts/bench-preflight.sh` — the `elif` arm prints
  and falls through to the verdict with `refused` untouched. And
  `cat /sys/fs/cgroup/memory.max` fails with no such file, which is why
  a walk that reaches the root finds nothing to read. Inside a scope,
  check 4's own line is the test: *"MiB under memory.max on the scope"*
  is a reading, *"reading unavailable"* is not.
- **Still open:** the *"reading unavailable"* branch wants revisiting,
  because the sentence it means has changed. It was written for a host
  with no cgroup v2 and now also fires for a host that put itself
  outside every ceiling — two situations with opposite risk, one
  message, and no refusal. Filed as a board row, not fixed here: this
  was a prose task and the clause is a script.
- **Replaces:** nothing. It is the condition
  `build-weight-off-root.md` does not cover: that lesson is about where
  a build tree is written, this one about a ceiling a phase escaped.
