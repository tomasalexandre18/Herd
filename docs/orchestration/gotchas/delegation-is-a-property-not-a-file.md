# Delegation is a unit property; the files present never say it

- **Date:** 2026-10-30, two instances in one probe, the second of them a
  correction that was itself wrong.
- **Observed, first:** the orchestrator wrote that
  `user-1002.slice` delegates, and confirmed it by reading that slice's
  `cgroup.subtree_control`. It does not. Measured:

  | read | value |
  |---|---|
  | `systemctl show user-1002.slice -p Delegate` | `Delegate=no` |
  | `… -p DelegateControllers` | empty |
  | `user-1002.slice/cgroup.subtree_control` | `cpu io memory pids` |
  | `systemctl show user@1002.service -p Delegate` | `Delegate=yes` |
  | `… -p DelegateControllers` | `cpu memory pids` |

  The slice's `subtree_control` is full and the slice delegates nothing.
  What delegates is **`user@1002.service`**. The wrong property was read
  and the answer was reported with confidence.
- **Observed, second — and this is the half worth the file.** The same
  report then corrected itself: *"`io` is not in `DelegateControllers`
  and reaches the scope anyway, because `io` is enabled in `app.slice`'s
  own `subtree_control`."* Re-read at rest on the same host, the same
  day: `app.slice`'s `subtree_control` is **`memory pids`**, and
  `app.slice`'s `cgroup.controllers` is **`cpu memory pids`** — `io` is
  not even available there to enable. The chain stops one level up:
  `user-1002.slice` passes `io` down, `user@1002.service` does not,
  exactly as its `DelegateControllers=cpu memory pids` says. So a
  correction of a file-reading error was made by reading another file.
- **Why:** a controller's files appear in a cgroup because its *parent*
  enabled that controller, and some files appear with no controller at
  all. The known-bad fixture is on this host and needs nothing built:
  `app.slice` holds `io.pressure` while `io` is absent from its
  `cgroup.controllers`. PSI files exist regardless, so "an `io.*` file
  is here" is the reading that manufactures both errors above.
- **Expected now:** to answer *does this delegate*, read the unit's
  `Delegate` and `DelegateControllers`, on the unit that would delegate,
  and nothing else. `cgroup.subtree_control`, `cgroup.controllers` and
  the presence of a knob answer a different question — *which
  controllers act here* — and they answer it for the level you read, not
  for the level below. State which property was read, in the sentence
  that makes the claim.
- **Verify:** read the property, then prove the detector fires:

```sh
systemctl show user@1002.service -p Delegate -p DelegateControllers
A=/sys/fs/cgroup/user.slice/user-1002.slice/user@1002.service/app.slice
ls "$A" | grep "^io\."      # prints io.pressure
cat "$A/cgroup.controllers"  # prints: cpu memory pids
```

  An `io.*` file is present and the `io` controller is not. A method
  that calls that delegation is a method that reads the host wrong.
- **What is still unknown:** whether `io` ever did reach the scope. The
  observation cannot be re-made without creating a scope, which the
  prose task that filed this was forbidden to do. Either systemd widened
  the subtree while the scope lived and trimmed it after, or an `io.*`
  file was read as a controller. **Nothing in the tree should assume
  `io` is delegated here**, and the io assertions' own skip reasons
  already require a block device as well.
- **Replaces:** nothing. Companion to
  the per-machine auto-memory note *verify the correction too*, of which this
  is the versioned instance: an agent correcting a reading is usually
  right, and the second reading has to be a different *kind* of reading
  or it repeats the first error with new confidence.
