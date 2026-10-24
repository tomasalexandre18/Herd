# Herd's Claude state root

Where a Herd Claude Code session keeps its state, how to start one, and
why it is never started by chatOnS's launchers. The scripts are
`ops/agents/env.sh` (sourceable) and `ops/agents/session.sh` (the
launcher). The reasoning is D174.

## Where the state lives

Everything a session writes is under one root, `/mnt/ssd_980/herd`.

| Variable | Path |
|---|---|
| `HERD_STATE_ROOT` | `/mnt/ssd_980/herd` |
| `HOME` | `/mnt/ssd_980/herd/claude/home` |
| `CLAUDE_CONFIG_DIR` | `/mnt/ssd_980/herd/claude/config` |
| `XDG_CONFIG_HOME` … `XDG_STATE_HOME` | under `$HOME` |
| `XDG_RUNTIME_DIR` | `/mnt/ssd_980/herd/claude/run` |
| `TMPDIR`, `TMP`, `TEMP` | `/mnt/ssd_980/herd/claude/tmp` |
| `CARGO_HOME` | `/mnt/ssd_980/herd/cargo` |

`env.sh` creates each of them and **refuses to continue** if any one
resolves outside the root. The checkout itself is not state and stays
where it is; `HERD_REPO` points at the checkout the script was read
from, so a worktree's copy starts a session in that worktree.

## Starting a session

```sh
/mnt/ssd_980/herd-project/ops/agents/session.sh          # a session
/mnt/ssd_980/herd-project/ops/agents/session.sh --continue
source /mnt/ssd_980/herd-project/ops/agents/env.sh       # just a shell
```

`session.sh` refuses to run as root, finds the Claude Code binary on
`PATH` or at `HERD_CLAUDE_BIN`, and says so loudly when the config
directory holds no `.credentials.json` — see the migration below, which
has to put one there before the first session.

## Why Herd is not started by chatOnS's scripts

`/mnt/ssd_980/chatons/wt/main/ops/agents/env.sh` exports
`CLAUDE_CONFIG_DIR=$CHATONS_ROOT/claude/config`. A Herd session started
through it therefore reads chatOnS's **user** settings, and two things
follow that have both already happened:

1. That `settings.json` carries an `autoMode` block whose `environment`
   says the session is chatOnS's orchestrator and whose `allow` list
   grants `sudo systemctl`, `nixos-rebuild switch`, `docker compose` and
   writes under `/etc/nixos`. Herd is shipped, not operated (invariant 9,
   D161): it must not hold those permissions at all, and a permission
   that exists only because of whose directory we are standing in is one
   nobody decided to give us.
2. Auto memory is keyed by the configuration directory. When the config
   directory moved on 2026-10-23, the two 2026-10-15 cards under
   `/home/server/.claude/projects/-mnt-ssd-980-herd-project/memory/`
   stopped being loaded without anything failing or saying so.

Both are properties of *whose* configuration directory we run in, not of
what the session is told. Telling a session to ignore a persona does not
remove a permission, and no instruction brings back a memory card that is
not in the directory being read. The remedy is the directory, which is
what this page is about.

So: never `source …/chatons/…/env.sh` for Herd work, and never run a bare
`claude` for Herd work either — a bare one inherits `/home/server/.claude`
and that is the directory the 2026-10-15 cards were lost from.

Sharing the Claude Code *binary* with chatOnS is fine and intended: it is
a tool, not state. `HERD_CLAUDE_BIN` exists for exactly that.

## What is deliberately not taken from chatOnS

chatOnS's launchers carry a cgroup slice (`chatons.slice`, `in-slice.sh`),
a private tmux server, a `dtach` orchestrator socket with `attach.sh` and
`restart.sh`, a Nix watcher, a Remote Control relay, an MCP daemon, a
boot `systemd` unit, and `HC`. None of it is here. Herd has no slice on
this host, no fleet of long-lived agent sessions to supervise and
re-attach, and no service of its own to start at boot; a Herd session is
started by a person or by an orchestrator that already has a terminal.
Adding a supervisor would be a second thing to keep working, and the
first thing to drift.

## Not done here: creating the root, and moving the data

`/mnt/ssd_980` is `alexandre:961`, mode `drwxrws--x`, and `server` is not
in group 961, so **`server` cannot create `/mnt/ssd_980/herd`**. The
first command needs the owner. Run the rest as `server`.

```sh
sudo install -d -o server -g users -m 755 /mnt/ssd_980/herd
install -d -m 700 /mnt/ssd_980/herd/claude \
  /mnt/ssd_980/herd/claude/config /mnt/ssd_980/herd/claude/home \
  /mnt/ssd_980/herd/claude/tmp

# 1. Credentials, before the first session (see "sync" below).
install -m 600 /mnt/ssd_980/chatons/claude/config/.credentials.json \
  /mnt/ssd_980/herd/claude/config/.credentials.json

# 2. Memory: merge the two scattered sets, newest first, never overwrite.
old=/home/server/.claude/projects/-mnt-ssd-980-herd-project
cur=/mnt/ssd_980/chatons/claude/config/projects/-mnt-ssd-980-herd-project
dst=/mnt/ssd_980/herd/claude/config/projects/-mnt-ssd-980-herd-project
install -d -m 700 "$dst/memory"
cp -n "$cur"/memory/*.md "$dst"/memory/
cp -n "$old"/memory/*.md "$dst"/memory/

# 3. Transcripts, so --continue and --resume still find the history.
cp -n "$cur"/*.jsonl "$dst"/ && cp -n "$old"/*.jsonl "$dst"/
cp -rn "$cur"/*/ "$dst"/ 2>/dev/null; cp -rn "$old"/*/ "$dst"/ 2>/dev/null
```

Then **write a new `settings.json` by hand**; do not copy chatOnS's.

```json
{
  "theme": "dark",
  "skipWorkflowUsageWarning": true,
  "inputNeededNotifEnabled": true,
  "agentPushNotifEnabled": true
}
```

Verify from inside the first session: `/status` names
`/mnt/ssd_980/herd/claude/config` as the settings source and `/memory`
lists the merged cards. From outside:
`tr '\0' '\n' < /proc/<pid>/environ | grep -E '^(HOME|CLAUDE_CONFIG_DIR)='`.

What a careless run of that procedure loses:

- **The two 2026-10-15 cards again.** `cp -n` in step 2 is what keeps
  them: the two sets share a `MEMORY.md`, and a plain `cp` of the newer
  set over the older one drops `orchestrate-do-not-code.md` and
  `herd-cargo-needs-nix-develop.md` from the index even when the files
  are present. After the copy, `MEMORY.md` must be edited by hand so it
  lists every card in the directory — a card no index names is a card no
  session reads, which is the 2026-10-23 failure in a new costume.
- **The permissions, if `settings.json` is copied.** Copying it carries
  the `autoMode` block across and reproduces the incident exactly.
- **Nothing, if the transcripts are skipped.** They are about 60 MB and
  only `--continue` and `--resume` need them. The same session id exists
  in both directories with different lengths; copying the chatOnS one
  first and `-n` second keeps the longer.
- **An hour of downloading**, if `CARGO_HOME` is not seeded:
  `cp -a /mnt/ssd_980/chatons/cargo/registry /mnt/ssd_980/herd/cargo/`
  avoids refetching the registry. Optional, and needs the network.

`.claude.json` is deliberately not copied: it holds chatOnS's project
list and MCP registrations. The cost is re-accepting the trust prompt
once.

## Not done here: the credentials sync service

`claude-creds-sync.service` (NixOS, outside this repository) keeps
exactly two files equal:

```
CREDS_A=/home/server/.claude/.credentials.json
CREDS_B=/mnt/ssd_980/chatons/claude/config/.credentials.json
```

Its `sync_once` returns immediately unless **both** files exist and are
non-empty, and it `inotifywait`s the two parent directories. A third
config directory is invisible to it, which is why the migration copies
the credentials by hand and why the first Herd session fails without
that copy.

For the owner, or for chatOnS, to generalise it to three paths:

1. Replace `CREDS_A` and `CREDS_B` with one list, e.g.
   `Environment="CREDS=/home/server/.claude/.credentials.json
   /mnt/ssd_980/chatons/claude/config/.credentials.json
   /mnt/ssd_980/herd/claude/config/.credentials.json"`.
2. In `sync_once`, keep the existing non-empty members of `$CREDS`, pick
   the one with the greatest `stat -c %.9Y`, and copy it over every other
   member whose content differs, with the same `mktemp` + `chmod 600` +
   `mv -f`. Keep the current rule that a **missing** file is never
   created: a config directory nobody has logged into should not be
   handed a token by a background service.
3. Pass every `dirname` to the single `inotifywait`, and make the `case`
   in the read loop match every member of `$CREDS`.
4. `inotifywait` fails on a directory that does not exist and
   `Restart=always` would then loop, so the unit needs the directory to
   exist first: a `systemd.tmpfiles.rules` entry for
   `/mnt/ssd_980/herd/claude/config` (`d … 0700 server users`), or an
   `ExecStartPre` that creates it. `RequiresMountsFor=/mnt/ssd_980` is
   already there and covers the new path.

One consequence to accept before applying it: the three files then hold
one token, so a `/logout` in any session logs all three out, and the
newest file always wins. That is already true of the two.
