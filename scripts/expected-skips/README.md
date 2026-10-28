# The assertions a host cannot make, as a list and never as a count

`scripts/gate.sh` compares the skips of a run against the list for the host
it ran on. It is red on any skip that is not on the list, and it says
nothing about how many there are.

Why a list and not a count. D27 says a skipped assertion is printed as
skipped and never reported as passed. A count cannot carry that: this
workstation skips 12 at rest and 14 under load, GitHub Actions skips 14, and
one red CI run on 2026-10-27 printed "AT LEAST 15" because it had not
finished. None of those four numbers means anything on its own. The name of
an assertion that was not checked does.

Why these files are here and not under `docs/`. `scripts/check-docs.sh`
width-checks every `docs/*.md` at 79 columns and a skip line is far longer
than that. They are evidence, not prose.

| file | what it is |
|---|---|
| `local.txt` | the list this workstation skips at rest. Produced by `./scripts/gate.sh --bless` from a run whose last line was `test suite: PASSED`; the header records the host and the load1 it measured on. |
| `local-under-load.txt` | the skips additionally allowed when load1 is above `HERD_GATE_LOAD_NOTE` (8). The two flood benches skip themselves above that load, so under load the list grows by exactly those two and the gate stays green without ever reading a count. |
| `ci.txt` | what `ubuntu-latest` skipped on green run 40564336703, for comparison. The gate does not read it on this host; `HERD_GATE_EXPECTED` can point at it on a host shaped like that one. |

The gate compares the assertion **name** of each line: the text between
`HERD-SKIPPED: ` and the first colon. The reason that follows is kept for
the reader and is deliberately not compared, because it carries a temporary
directory and a pid that change on every run.

Updating a list is a decision, not a chore: a new name on it is an
assertion nobody checks any more. Say in the landing record why it is
there.
