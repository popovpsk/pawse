# autoeq

The AutoEq client: the list of headphone profiles (`results/INDEX.md` in
`jaakkopasanen/AutoEq` on GitHub) and one profile's `ParametricEQ.txt`. Blocking `ureq`;
the app calls it on `net_runtime`'s pool.

Not here: reading the profile text (`equalizer::parse`, `equalizer/src/doc.md`), search and
the picker (`pawse/src/tools/equalizer/doc.md`).

## Files

- `lib.rs` — `Client` (`index`, `parametric_eq`), `Entry` (`cache_name`), `parse_index`,
  `Error`, tests.

## Spec

- An index line is `- [Name](./source/rig/Folder) by Source[ on Rig]`. The link is taken
  up to the last `) by `, because model names have parentheses. Lines that do not fit, and
  paths with `..`, are skipped.
- The profile lives at `<path>/<last path segment> ParametricEQ.txt`. The index already
  percent-encodes the path, so it is joined as is.
- Nothing is cached here. `cache_name` is the flat file name the app keeps a profile
  under (the path with `/` as `%2F`, plus `.txt`); `None` when the path holds a character
  outside a small set every file system accepts, or is too long, since the index comes
  from the network.

## Decisions

Owner's calls; ask before changing.

- Downloaded on demand from GitHub, not bundled: the list stays current and the
  measurements are not redistributed with the app.
- Only the parametric profiles; the graphic EQ, convolution and HeSuVi outputs are not
  used.
