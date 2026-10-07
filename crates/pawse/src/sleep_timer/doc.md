# sleep_timer

The sleep timer: pauses playback after a chosen time or at the end of the
current track. It is armed by hand on the Tools screen (Timer page) or by
itself at night (the automatic timer, Settings → General). While armed, a small
badge with the remaining time is shown in the footer, in the volume row left
of the volume icon (between the track length and the volume); clicking it opens
the Timer page. It is drawn by `footer.rs` (`sleep_timer_badge`); the footer
observes the `SleepTimer` entity and emits `OpenSleepTimerEvent`, which
`MainView` turns into opening the Timer page. Being in the footer, the badge is
also there in full screen, but hidden with the rest of the chrome in cover mode.
The free part of the volume row is narrow (the right column is `200 × scale`
wide, the volume icon and slider take 130 px), so only the countdown shows its
text; the end-of-track mode is a bare moon (`SleepTimer::is_countdown`) whose
tooltip carries the status, because the localized "End of track" does not fit.

## Files

- `mod.rs` — the `SleepTimer` entity (a global, created by `setup` before the
  first window): armed state, precomputed badge / status texts, the tick task,
  fade-out and the automatic arming. `stop_at_track_end` is the hook
  `services::advance_on_track_end` asks before starting the next track.
- `schedule.rs` — pure functions: which night a moment belongs to
  (`window_opened_on`), minute-of-day ↔ `NaiveTime`, the remaining-time,
  end-time and clock labels, the effective fade length (`fade_window`), the fade
  gain curve and the tick spacing. Tests live here.
- `controls.rs` — `SleepTimerControls`, the entity holding the window-bound
  editor states: the from / until `TimeFieldState`s, the auto-duration and
  manual-duration number inputs and the fade-length slider. `MainView` creates
  it once and hands the same entity to the Settings pages and to `ToolsView`, so
  the fade slider shown on both screens is one state and can't drift. Its
  subscriptions write the settings; the manual minutes live only here (not
  saved). Both screens observe it, because `TimeField` and `Slider` read their
  state while the parent renders and don't redraw on their own.
- `settings.rs` — the "Sleep timer" group of Settings → General, three rows:
  the automatic timer switch with its from–until window on the same line, the
  duration, and the fade-out row (length slider + switch) shared with the Tools
  page; plus `duration_field` (number input + "min"), also used by Tools. The
  group carries `SETTINGS_ANCHOR`: the Tools page's "Settings" button
  dispatches `settings_view::OpenSleepTimerSettings`, and `MainView` opens the
  General page scrolled to this group. `SleepTimerSettings` itself lives in
  `settings_store.rs`.

The Tools page is `tools/timer.rs`.

## Behaviour

- **Wall-clock countdown.** A timer counts real time, not playback time: a
  manual pause does not stop it. When it runs out while nothing plays it just
  turns off.
- **Ticks only while armed.** The task wakes on whole-second boundaries of the
  remaining time (so the badge changes once a second), every `fade_tick` during
  the fade (the fade / 120, 250 ms – 1 s: an hour-long fade needs no faster
  updates than the badge, and 4 redraws a second for an hour would be waste),
  and never when the timer is off or armed for end of track.
- **Fade-out** (on by default) lowers the output gain over the last
  `fade_secs` (default 30) with a squared curve (linear amplitude sounds like
  it drops only at the very end). The length is one of
  `SLEEP_TIMER_FADE_STEPS` (5 s … 1 h, `settings_store.rs`): a roughly
  geometric scale (~1.25× per step) of round values, so the slider is fine at
  the short end and coarse at the long end; the slider value is the step index.
  Under 2 min the label is in seconds, from 2 min in whole minutes (every step
  from 120 s up is a whole minute). `sanitized` snaps any stored value to the
  nearest step, so a settings file from before the slider (no `fade_secs` →
  default 30) or a hand-edited one always matches a slider position. The length is taken when the
  timer is armed or extended and capped by `span` (the countdown length as
  armed, or as left after +5 min): a 1-minute timer with a 2-minute fade starts
  at full volume and fades over the whole minute instead of starting already
  quieter. Moving the slider while a timer runs is picked up on a tick only
  while neither the old nor the new fade has begun (`settled_fade`): shortening
  mid-fade would snap back to full volume, lengthening into the remaining time
  would drop it at once. Switching fade off applies immediately (explicit, and
  it restores the volume); switching it on inside the would-be window does
  nothing for this timer. With fade off the window is zero, so there are no
  fast ticks at the end. It sets `Output` volume directly and never touches the saved
  volume, so the volume slider does not move and a restart can't persist a
  faded volume. After the pause the full volume comes back `RESTORE_DELAY` later
  (the engine's own pause fade must finish at the low gain first) or at once
  when playback starts again. Extending or cancelling during the fade restores
  it immediately. A manual pause leaves the gain where it is: the engine's own
  pause fade is still running at that gain, and on resume the fade simply goes
  on — restoring on pause gave a loud blip in both places. Exclusive mode has no
  software volume, so there the timer only pauses.
- **End of track** is not a deadline: `advance_on_track_end` asks
  `stop_at_track_end`, and when the timer is armed that way it loads the next
  track paused instead of playing it, so Play in the morning continues with it.
  A manual skip doesn't trigger it; the first track that ends by itself does.
- **Automatic timer.** Arms for the configured duration on an engine `Playing`
  event (play, resume, every new track) when nothing is armed and the local time
  is inside the from–until window. The window may cross midnight; equal bounds
  mean the whole day. So a session that started before the window opens gets
  the timer at the next track change. After the timer runs out and the user
  plays again, it re-arms — they fell asleep again. **Turning a timer off**
  inside the window (auto or manual) suppresses the automatic one until the
  next night; the night is identified by the date the window opened
  (`window_opened_on`), so 02:00 belongs to the previous evening.
- **Countdown in the badge, clock time in Tools.** The footer badge shows
  the time left (a glance while falling asleep); the Tools status shows when it
  will pause ("Pauses at 23:47"), which is what one plans by. The end time is
  recomputed from the local clock on every relabel rather than stored, so a
  wall-clock change can't leave it stale; it drops the seconds, matching the
  fade start for whole-minute presets.
- The status line in Tools says when a timer was started automatically, so an
  unexpected pause is explainable.
- **Picking values.** Times are `TimeField`s (type `2330`, or ↑/↓ on the
  selected segment), not a menu of half-hour steps; every edit is saved at
  once, so a half-typed hour is briefly stored. The window is read only on a
  `Playing` event, so a stray automatic arm needs a track to start in the
  second between two digits — accepted: the badge shows it and Turn off undoes
  it, while saving only on focus-out could lose an edit when the overlay closes
  with the field focused. Durations are number inputs, 1–720 min: typing saves
  each in-range value, −/+ snap to multiples of 5, and Enter or leaving the
  field commits the text clamped to 1–720 (`committed_minutes`; empty or not a
  number puts the last valid value back). Presets stay on the Tools page for the
  one-click case; the field next to them (Enter or Start) covers the rest.
