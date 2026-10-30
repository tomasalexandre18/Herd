# A `\x{…}` escape in perl recodes the whole file

- **Date:** 2026-10-30, on the branch `shared-host-and-selectors`; written
  down by `host-ownership-rule` after reproducing it.
- **Observed:** a one-word substitution in `CLAUDE.md` written as
  `perl -i -0777 -pe 's/…/…\x{2013}…/'` — an en dash as a Unicode
  escape — left **four unrelated lines** of the file at 80 to 82 columns
  where they had been 78 to 79, and `check-docs.sh`'s width check was the
  only thing that caught it. Nothing in the diff of the edited sentence
  was wrong.
- **Why:** `\x{…}` above 0xFF upgrades perl's record from bytes to
  characters. The bytes already read from the file are then reinterpreted
  one byte per character and re-encoded on output, so **every multibyte
  character already in the file is double-encoded**: an em dash
  `\xe2\x80\x94` is written back as `\xc3\xa2\xc2\x80\xc2\x94` — three
  bytes become six, one character becomes three, and the line gains two
  columns. A file of prose at 79 columns with two dashes on a line is
  over the limit by arithmetic alone. Reproduced on 2026-10-30 on this
  host with perl 5.42.0: a four-line fixture holding `—` and `≤` went
  from 81 bytes to 90, line 2 from 23 columns to 25 and line 3 from 17 to
  19, and perl printed `Wide character in print` once.
- **Expected now:** write the replacement as **raw bytes** under
  `LC_ALL=C` — `s/…/\xe2\x80\x93/` — which leaves the record in bytes and
  touches nothing else, or use the file tools and type the character.
  `Wide character in print` on stderr means the damage is already in the
  file; `git checkout -- <file>` and do it again.
- **Verify:** after any `perl -i` over a prose file,
  `git diff --stat` against the byte count you expect, and
  `./scripts/check-docs.sh`. The direct reading is
  `LC_ALL=C git diff -- <file> | grep -c $'\xc3\xa2\xc2\x80'`, which is
  zero in a healthy tree and nonzero for every double-encoded dash.
- **Replaces:** nothing. It is the mechanical half of
  `invariants-are-quoted-not-reflowed.md`: that lesson is about text
  removed on purpose to save columns, this one about columns appearing on
  lines nobody edited.
