# Own scratchpad, no pkill

- **Date:** 2026-10-23 (memory `subagent-brief-hygiene.md`).
- **Observed:** two agents both wrote `scratchpad/gates.log`; one read
  back the other worktree's gate output and nearly reported it as its
  own. The same day an agent ran `pkill -f herd-runner`, which killed its
  own shell and a neighbour's running test binaries.
- **Why:** parallel agents share the machine and the session scratchpad
  and cannot see each other. `pkill -f` matches the invoking shell's own
  command line and every sibling's binaries.
- **Expected now:** every agent writes under its own scratchpad
  subdirectory and its own worktree only; kills only PIDs it started; and
  reports a collision instead of working around it.
- **Verify:** the brief names the scratchpad subdirectory; the handoff
  names the PIDs or says none were started.
- **Replaces:** nothing.
