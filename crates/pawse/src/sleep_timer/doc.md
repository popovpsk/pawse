# sleep_timer

The sleep timer: pauses playback after a chosen time or at the end of the current track,
armed by hand on the Tools Timer page or by itself at night (Settings → General).
`SleepTimer` is a global entity created by `setup` before the first window; `timer(cx)`
returns it as an `Option`.

Not here: the Timer page (`tools/timer.rs`, `tools/doc.md`); the footer badge (`footer.rs`);
`SleepTimerSettings` and `SLEEP_TIMER_FADE_STEPS` (`settings_store.rs`);
`services::advance_on_track_end`, which asks `stop_at_track_end`.

## Files

- `mod.rs` — `SleepTimer`: armed state, precomputed badge / status texts, the tick task,
  fade-out, automatic arming.
- `schedule.rs` — the pure parts, with their tests: `window_opened_on`, labels, fade window,
  gain curve, `settled_fade`, tick spacing.
- `controls.rs` — `SleepTimerControls`: the window-bound editor states (time fields, duration
  inputs, fade slider). `MainView` creates one and hands it to the Settings pages and to
  `ToolsView`, so the fade slider on both screens is one state. Both screens observe it,
  because `TimeField` and `Slider` read their state while the parent renders and do not
  redraw on their own.
- `settings.rs` — the "Sleep timer" group of Settings → General, plus `duration_field` and
  `fade_item`, which Tools reuses. The group carries `SETTINGS_ANCHOR`: the Tools page's
  "Settings" button dispatches `OpenSleepTimerSettings` and `MainView` opens General scrolled
  to it.

## Behaviour

- The countdown is wall-clock time: a manual pause does not stop it. If it runs out while
  nothing plays, it just turns off.
- `is_countdown()` is false for end of track: the footer draws a bare moon with the status in
  its tooltip, because the localized "End of track" does not fit.
- End of track is not a deadline: `stop_at_track_end` makes the next track load paused
  instead of playing, so Play in the morning continues with it. A manual skip does not
  trigger it; the first track that ends by itself does.
- Automatic timer: arms on an engine `Playing` event (play, resume, every new track) when
  nothing is armed and the local time is inside the from–until window. The window may cross
  midnight; equal bounds mean the whole day. A session that began before the window gets the
  timer at the next track change. After a timer runs out, playing again re-arms it.
- Turning a timer off inside the window (automatic, manual or end of track) suppresses the
  automatic one until the next night. The night is the date the window opened
  (`window_opened_on`), so 02:00 belongs to the previous evening.
- The end time ("Pauses at 23:47") is recomputed from the local clock on every relabel, not
  stored, so a clock change cannot leave it stale.

## Fade-out

- Lowers the output gain over the last `fade_secs` with a squared curve, because linear
  amplitude sounds like it drops only at the very end.
- The slider value is an index into `SLEEP_TIMER_FADE_STEPS`; `sanitized` snaps any stored
  value to the nearest step. Labels are in seconds under 2 min, in whole minutes from there.
- The length is capped by `span` (the countdown as armed, or as left after +5 min): a 1-minute
  timer with a 2-minute fade fades over the whole minute from full volume.
- A slider change while a timer runs is picked up only while neither the old nor the new fade
  has begun (`settled_fade`): shortening mid-fade would snap back to full volume, lengthening
  into the remaining time would drop it at once. Switching fade off applies immediately and
  restores the volume; switching it on inside the would-be window does nothing for this timer.
- It sets the `Output` volume directly and never touches the saved volume, so the volume
  slider does not move and a restart cannot persist a faded volume.
- No fade while the output is exclusive or a cast target is active; the timer then only pauses.
- After the pause the full volume returns `RESTORE_DELAY` later (the engine's own pause fade
  must finish at the low gain first) or at once when playback starts. Extending or cancelling
  during the fade restores it at once.
- A manual pause leaves the gain where it is: the engine's own pause fade is running at that
  gain, and on resume the fade goes on.

## Decisions

Owner's calls; ask before changing.

- The badge shows the time left, the Tools status the clock time it will pause at — a glance
  while falling asleep versus what one plans by.
- Times are `TimeField`s, not a menu of half-hour steps. Every edit is saved at once, not on
  focus-out, which could lose an edit when the overlay closes with the field focused. A
  half-typed hour can briefly arm a stray automatic timer only if a track starts in that
  second; accepted, since the badge shows it and Turn off undoes it.
