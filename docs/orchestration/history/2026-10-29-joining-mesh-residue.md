# joining-mesh-residue — handoff 2026-10-29

- **Item:** none named in the brief; a board row is proposed below
  (next free id ORCH-146)
- **Agent:** herd-step
- **Branch / commit:** `joining-mesh-residue` / `9c2267e`, based on
  origin/main `09229db`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`docs/joining.md` gains one section at the end, after "Picking `ws://` or
`wss://`, and the certificate that decides it": **"What protects a pack
between runners, and what does not"** (76 added lines, nothing else in
the repository touched).

What it states, for the person deciding whether their mesh is good
enough:

- **Not built.** No pack frame is numbered on either protocol enum, no
  fetch endpoint binds anything, `keeper` appears nowhere in the
  binaries; M11 builds it. The section is explicit that what follows is
  a property of the design being built, decided before the first frame
  was numbered (Alexa, 2026-10-29), not shipped behaviour.
- **Herd gives authorisation, not confidentiality**, as a two-row table
  naming who provides each.
- **The ephemeral capability:** the hub mints a random bearer token and
  pushes it to the holder over the holder's own control link — the one
  already authenticated with the token of step 5 — saying expect this
  token, for this pack, from this peer, for the next few minutes. No
  key, no certificate, no CA, no PKI, nothing secret in the repository
  (invariant 7). A fetch with no valid capability is refused.
- **What the capability buys**, said so a reader cannot read it as
  useless: a machine the hub never authorised cannot fetch a pack, even
  reaching the serving address and knowing what to ask for.
- **Why no TLS of Herd's own:** TLS asks who signs, who verifies, how a
  key reaches each machine and what happens when one expires — a
  trust-distribution subsystem to answer "someone is already inside your
  private network", and invariant 1 refuses the surface.
- **The residue, named:** on a mesh that does not encrypt, pack bytes
  travel in clear, and the capability is replayable by anyone who can
  read it for as long as it lives. Then the one question it leaves the
  operator, with yes for a tailnet or WireGuard and no for a flat LAN, a
  shared VLAN or a mesh somebody else operates — and the fix is the
  mesh, not a Herd flag, because there will not be one.
- **Invariant 3 is pointed at, not repeated:** the closing paragraph
  sends the reader to this document's own "What you never need" list and
  adds only what the pack path does not bend — the endpoint binds the
  single named mesh interface, is refused at startup on a default or
  public one, control is still outbound, never a forwarded port
  (invariant 3, D11).

Voice and shape follow the document: bold lead-in sentences, second
person for the joining machine's owner, "operator" kept for the person
at the hub, D-numbers cited inline, no heading renamed and no section
moved.

## Not done, and why

- **No Rust gate run**, and none needed: the change is prose under
  `docs/` only. See *Gates*.
- **No entry in `docs/decisions.md`, no board row, no `docs/status.md`
  edit** — the merger owns all three; the proposed texts are in
  *Decisions needed* and *Risks and findings* below, in this handoff
  only, and `status.md` (90/90) and `CLAUDE.md` (150/150) were not
  opened.
- **Nothing copied into `docs/architecture.md`, `docs/plan.md` or
  `docs/runner-confinement.md`**; what each of them already says, and
  the one wording I would change in a later task, is under *Risks and
  findings*.

## Gates

Run before this file was committed, on the tree holding only the
`docs/joining.md` change:

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

Exit status 0. That `note:` line is pre-existing on `origin/main` and is
a note, not a failure.

**The Rust gates — `cargo build --workspace`, `./scripts/test.sh`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all --check` — were NOT RUN locally**, and are reported as
not run, never as passed. The reason is the one in CLAUDE.md
*Commands*: this change touches only `docs/`, so the docs gate is what
it needs, and the Rust gates are covered by the batch's own
`./scripts/gate.sh` run at integration, which is `herd-merger`'s. The
brief also forbade any build or suite run on this host while another
agent holds the heavy work. `./scripts/gate.sh` was not run either, for
the same reason.

**Last run, named and not quoted:** `./scripts/check-docs.sh` on the
committed tree including this handoff; its verbatim output is in the
agent's report, not in this file.

Skips: no skip. Nothing in this task asserts anything, so there was no
`HERD-SKIPPED` line to copy.

## Risks and findings

- **`docs/architecture.md` does not conflict.** It already marks the
  whole pack half unbuilt and in the same words I used: §1's diagram
  edge reads "UNBUILT: peer-to-peer pack fetch on a private mesh
  interface — D10, D11, M11", line 106 says "The private-mesh fetch
  endpoint invariant 3 permits is not built", and its unbuilt table
  (line 1276) lists "no mesh-bound fetch endpoint, no same-host hardlink
  or reflink path, no keeper, no scoped fetch capability". It says
  nothing about confidentiality, bearer replay or what the operator owns
  — so the new section adds a property it does not state, rather than a
  second copy of one. **I duplicated none of it**; the section says
  "not built" in one short paragraph and sends the reader to
  `docs/plan.md` for M11.
- **`docs/runner-confinement.md` does not conflict either**, and is not
  the better home. Its one mention of the subject is the "No inbound
  socket" paragraph (line ~183): "A runner binds nothing in M4 […] The
  mesh-bound fetch endpoint of D10 and D11 arrives in M11." That is an
  M4 confinement fact about the shipped binary, true as written; a
  property of a transport that does not exist yet would sit oddly in a
  document whose subject is what one runner may do to its own host. My
  recommendation: **the property stays in `joining.md`**, which is the
  operator-facing document and the one place an operator reads before
  trusting their mesh.
- **`docs/plan.md` M11 is where the mechanism lives**, and it already
  has it in engineering terms: "it issues a short-lived, scoped fetch
  capability for a (blob, peer) pair", "A fetch endpoint binds to the
  one configured mesh interface", and in *Done when* "a fetch without a
  valid capability is refused". It does **not** say the bytes are in
  clear, nor that the capability is replayable, nor whose job
  confidentiality is. If anything should be added there later it is one
  sentence pointing at `joining.md`'s section rather than a second
  statement of it — a candidate task, not something I did, since M11's
  text is the milestone's own contract.
- **One wording that will go stale, and I left it alone on purpose.**
  `docs/joining.md`'s opening "What you never need" list says
  "nothing on your machine listens (invariant 3, D11)". That is exactly
  true today — M4 binds nothing — and becomes half-true the day the
  fetch endpoint lands, when the honest form is "nothing on your machine
  listens on a default or public interface". Changing it now would make
  the document describe an endpoint that does not exist, and the brief
  told me not to restructure the document; my new section's closing
  paragraph points at that list rather than contradicting it. **Finding
  for whoever builds M11: that bullet is part of the M11 docs change.**
- **The protocol surface the section implies is not numbered.** "the
  hub mints a token and pushes it to the holder over the holder's own
  control link" needs a `FromHub` variant that does not exist. The
  section is worded as design, not as behaviour, so it is not a false
  claim — but it is a claim M11 now has to satisfy, and a reader who
  diffs the two will hold us to it.
- **No risk to the machine.** No build, no suite, no `cargo`, no
  `gate.sh`, no process started.

## Decisions needed

**One: the `docs/decisions.md` entry, which only the merger writes.**
Proposed text, for the merger to paste under the number it allocates:

```markdown
## 2026-10-29 — D-NEXT Pack transfer: authorisation ours, confidentiality
the operator's

**Context.** M11 moves a job's declared artefacts runner to runner over
the machine owners' private mesh (D10, D11). Before any pack frame was
numbered, Alexa decided what that transfer protects, so the property
would be a stated one rather than something a reader infers from code.

**Choice (Alexa, 2026-10-29).** Herd provides **authorisation, not
confidentiality**.

1. The right to fetch a pack is an **ephemeral capability**: a random
   bearer token the hub mints and pushes to the holder over the
   holder's own already-authenticated control link — expect this token,
   for this pack, from this peer, for N minutes. There is no key to
   distribute, no PKI, and nothing secret in the repository
   (invariant 7). A fetch with no valid capability is refused.
2. **Confidentiality is the operator's**, supplied by the private mesh
   the runners are already on. Herd adds **no TLS of its own**, because
   TLS implies trust distribution — who signs, who verifies, how a key
   reaches a machine, what happens when it expires — a whole subsystem
   to answer "someone is already on your private network", and
   invariant 1 refuses that surface.

**The residue, taken knowingly.** On a mesh that does not encrypt, pack
bytes travel **in clear**, and the capability is **replayable by anyone
who can read it, for as long as it lives**. That is the cost of the
choice and Alexa accepted it with that sentence in front of him.

**Consequence.** `docs/joining.md` carries the operator-facing form in
"What protects a pack between runners, and what does not", written
before any pack code exists; invariant 3 is cited there, not restated.
M11 owes the capability's issue, its lifetime and its refusal path, and
owes the correction of this document's "nothing on your machine
listens" bullet when the fetch endpoint lands.
```

**Two: the board row**, for the merger to add:

```markdown
| ORCH-146 | M11 must satisfy what `joining.md` now states: the fetch capability is minted by the hub and pushed over the holder's own control link, is short-lived, and a fetch without one is refused — and the opening "nothing on your machine listens" bullet is corrected in the same change | `docs/joining.md` "What protects a pack between runners, and what does not", 2026-10-29; D-NEXT | orchestrator | open | the property is written; no frame carries it yet and `keeper` is still absent from the code | if M11 slips, the section stays correct as design — it says so — but the gap widens | `docs/joining.md`; `docs/plan.md` M11; `docs/architecture.md` line 1276 |
```

The id is the next free one on the board today (highest is ORCH-145);
the merger reallocates if another branch took it first.

## Resume point

Nothing to continue in this task. For whoever integrates:
`git fetch && git log --oneline origin/joining-mesh-residue -1` →
`9c2267e`, then the batch's own `./scripts/gate.sh` at integration.
The next piece of work is ORCH-146's proposal above, and it is M11's.

## Processes

None. No build, no test, no background job, no PID started or ended, and
`CARGO_TARGET_DIR` was never set because nothing was compiled; there is
no `target/` of mine to delete.
