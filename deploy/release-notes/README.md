# A release's own notes

One file per release, named for the version exactly as `Cargo.toml`'s
`workspace.package.version` carries it: `0.4.0.md`, not `v0.4.0.md`.

`.github/workflows/release.yml` reads
`deploy/release-notes/<version>.md` and puts it in the release body
**between** the half it generates — file names, `sha256sum`,
`RECORD_VERSION` and `PROTOCOL_VERSION`, the `docker load` lines — and
`deploy/release-notice.md`, the standing notice it quotes underneath.
So this file is the human half of the page and nothing else: it does not
restate the sums, the file names or the notice.

`scripts/check-docs.sh` requires the file for the version the workspace
carries. That is the point of the directory: v0.4.0 was published with
none of the notes written for it, because the step that supplied them
was a person remembering to paste (ORCH-162). A version bump that
forgets its notes now goes red on a branch, before a tag is pushed,
instead of after a page is public.

Files for versions that have already shipped stay here. They are not
checked and they are not an error — that history is the point.

If the notes for the current version are genuinely absent, a release
still publishes: the workflow writes that piece empty rather than
failing, because the body is never allowed to cost a tag its binaries
(D162). The guard against an empty half is the check on a branch, not a
failure in the release factory.
