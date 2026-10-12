# Herd

Distributed build and test execution, "GitHub-runners shaped", independent of
any particular project.

A **requester** asks for work on a commit — build, test, check — without
knowing which machine will run it. The **hub** picks a **runner** whose
declared capabilities fit, and returns the result and the logs. Runners dial
out to the hub, cap what each job may consume, and can be stopped by their
owner in seconds.

Herd stays specialized on purpose: no workflow language, no plugins, no cloud.

- Working agreement: [`CLAUDE.md`](CLAUDE.md)
- Current state: [`docs/status.md`](docs/status.md)
- Decisions: [`docs/decisions.md`](docs/decisions.md)
- Plan: [`docs/plan.md`](docs/plan.md)
