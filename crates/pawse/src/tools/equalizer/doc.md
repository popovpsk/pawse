# tools/equalizer

The Equalizer page, the first tab of the Tools screen: the on/off switch, a toolbar (the
profile dropdown with its state, Save, Headphones…), the curve with draggable band points
and the band table. Headphones… opens a dialog that searches AutoEq and previews a
correction before loading it. The page edits `UserSettings::equalizer` and feeds
`Services::equalizer`, the `equalizer::Control` every engine reads.

Not here: the filters, preamp and file format (`equalizer/src/doc.md`), where the signal is
processed (`audio_engine/src/doc.md`), the AutoEq download (`autoeq/src/doc.md`), hosting the
page (`tools/doc.md`), the bit-perfect reason (`audio_output/src/doc.md`).

## Files

- `mod.rs` — `EqualizerView` (curve, points, band table, add / remove all, profile
  state, the dropdown's label and state), the switch, the toolbar and the profile menu
  (profiles, Save as…, import / export, folder, delete), the save-as and delete dialogs,
  `import` / `export`, `page`.
- `profiles.rs` — the profile files: `folder`, `list`, `read`, `write`, `delete`,
  `clean_name`, `same_name` / `contains`, `normalized`.
- `graph.rs` — the axes (20 Hz–20 kHz log, ±15 dB), `curve`, `Plot`: grid and curve
  painting, the cached curve path; used by the editor and the dialog's preview.
- `picker.rs` — `HeadphonePicker` and `open`, the Headphones dialog: the AutoEq list
  grouped into models, fuzzy search, the measurement buttons, the preview, Load; the
  on-disk profile cache.

## Spec

- Settings are the truth for `enabled`, `bands` and `source` (the loaded profile's or
  file's name). `EqualizerView` keeps a working copy; `apply` pushes it to the `Control`
  (bands only while enabled) and sets `Output::set_equalized` at once, so a paused player's
  bit-perfect indicator follows the switch before the engine writes again; `commit` also
  saves. A drag pushes on every move and saves on release; typing, the menu and the
  checkbox save at once (the settings writer coalesces).
- Loading a profile, a model or a file replaces all bands and turns the equalizer on.
  Remove all also forgets `source`. Add band is disabled at `equalizer::MAX_BANDS`;
  `SettingsStore::load_from` cuts a hand-edited `settings.json` to it, so what plays is
  what the table shows.
- The curve is the response at 48 kHz (the engine designs at the track's rate; only the
  top octave differs). It is recomputed on a change, not in render; the stroked path is
  built at the origin and cached per plot size and curve revision, and each paint only
  shifts a copy, because lyon tessellation is the costly part of a paint and the plot
  moves on every frame of a page scroll.
- Band points are absolute divs over the plot canvas. A drag starts on a point's mouse
  down; moves and the release come from window-level listeners registered in the canvas
  paint, so a drag outside the plot keeps going and a release anywhere ends it. A move
  without the left button, or the next mouse down anywhere, ends a drag whose release was
  missed (the page closed mid-drag). Shift during a drag changes Q by the vertical
  distance (up is narrower); the wheel is left to the page, which scrolls under the plot.
- Each band row owns three `InputState`s, rebuilt whenever the band list is replaced or
  its length changes. A drag or a type change writes them with `set_value`, which emits
  no `Change`, so typing is never fed back into the field being typed in. Text that does
  not parse is ignored until it does; values are clamped to the `equalizer` limits, and
  the row is rewritten with what plays when a field loses focus, not while typing (a
  partial "5" on its way to "500" would be clamped to 10).

## Headphones dialog

- The list is downloaded when the dialog first opens and kept for the session (a failure
  can be retried). Entries are grouped into one model per exact name; a model's
  measurements are ordered oratory1990, crinacle, then the rest as listed (AutoEq's own
  preference). A row shows the first source and how many more; a check marks the model
  the editor's bands come from (its `source`).
- Opening clears the search, so the list holds every model, and selects the model and
  measurement the bands come from (scrolled to the middle); bands from elsewhere keep the
  previous selection. Typing selects the best match. Up / Down move the selection from the search
  field: they are caught in the capture phase, because the single-line input swallows
  them.
- Search is `crate::fuzzy` over model names, best first (equal scores keep the name
  order). It runs on the background executor; each keystroke bumps a generation and only
  the newest result is applied, so typing never waits for matching. "No headphones
  match" waits for that result instead of flashing for a stale empty one.
- A selection downloads its profile after `PREVIEW_DELAY`, so arrowing through the list
  does not fetch every row passed, and draws it in the preview. Results are kept for the
  session per (model, measurement); a failure until Try again or a language change, and
  opening the dialog refetches the selection's dropped one. Nothing reaches the editor
  before Load (the button, Enter or a double click), which replaces the bands and closes
  the dialog; pressed before the preview arrives, it waits for it, and a failure or
  closing the dialog drops the wait.
- A downloaded profile is written to `dirs::cache_dir()/pawse/autoeq/` under
  `Entry::cache_name` once it parses (so an error page is never kept) and is read from
  there first, so each measurement is downloaded once per machine. The files never
  expire: they are a few hundred bytes and rarely change upstream; one that no longer
  parses is downloaded again. The list itself is downloaded once per session.
- The list is a `uniform_list` (only visible rows are built) of a fixed height and
  contains its wheel (`contain_scroll`), since the dialog body scrolls on a short window.

## Profiles

- A profile is `<name>.txt` in `dirs::data_dir()/pawse/equalizer/`, written by
  `equalizer::write`, so it is an Equalizer APO / AutoEq file and any such `.txt` dropped
  into the folder is a profile. The name is the file stem; `clean_name` replaces path
  and reserved characters, trims dots and spaces, caps the length in bytes (the temp
  file must fit a 255-byte file name) and appends `_` to a Windows device name (`NUL`,
  `COM1`, …). Names compare case-insensitively except on Linux, matching the file
  systems. The folder is listed each time the menu opens (a few small files), so files
  added by hand show up.
- Settings keep the active band list and the profile's name; the file is what the
  profile *is*. On start and on selection the file is read on the background executor
  into `saved`; `dirty` (bands differ from `saved`) is recomputed with the curve, not in
  render. A profile whose file is gone or unreadable is dropped from the page on start;
  settings forget it on the next change.
- The dropdown names what plays: the profile, else the loaded model or file, else "No
  profile". Beside it, "edited" when the bands differ from the profile's file, "not
  saved" when there is no profile and some bands. Save writes into the profile, or asks
  for a name when there is none; it is never enabled with no bands (a file without
  filters cannot be read back).
- Save writes the bands rounded to the file's precision (`normalized`) and adopts the
  rounded values, so a fresh save is never "edited" after a restart. If the bands changed
  while the file was being written, the newer bands stay. Writes go to a hidden temp file
  and rename.
- Save as asks for a name (prefilled with the profile, else the loaded model or file);
  a name that belongs to another profile needs a second confirm, the warning in between.
  Enter confirms through the dialog's own binding (the field passes Enter on), so there
  is no `PressEnter` handler, which would confirm twice and skip the warning. The field
  is focused after the dialog opens, because opening it takes the focus. Delete asks
  first; a file already gone counts as deleted. Selecting a profile, like loading a
  model or a file, replaces the bands and turns the equalizer on; loading a model or a
  file detaches the bands from the profile (it is unsaved work).
- Reads and writes run on the background executor. `epoch` is bumped by every load,
  selection and save; a result that comes back after a newer one started is dropped,
  so a slow write cannot attach a profile to bands loaded meanwhile.

## Decisions

Owner's calls; ask before changing.

- The first Tools tab.
- One global band list, no per-device profiles: saved profiles are switched by hand.
- Profiles are plain files in the data folder (next to the library and logs), chosen
  from a dropdown; no profile table in the database.
- One place says what plays: the profile dropdown and its state. Import, export and the
  rest of the file handling live in its menu.
- Headphone search is a dialog: search, pick a measurement, see the preview, then Load.
- Short texts: no paragraphs explaining the page.
- No equalizer mark in the player bar.
