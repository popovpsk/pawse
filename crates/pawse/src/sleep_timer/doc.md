# sleep_timer

The sleep timer: pauses playback after a chosen time or at the end of the
current track. It is armed by hand on the Tools screen (Timer page) or by
itself at night (the automatic timer, Settings → General). While armed, a small
badge with the remaining time sits in the window title bar, above the search
field; clicking it opens the Timer page. In full screen the title bar is a
thin inset, so there is no badge.

## Files

- `mod.rs` — the `SleepTimer` entity (a global, created by `setup` before the
  first window): armed state, precomputed badge / status texts, the tick task,
  fade-out and the automatic arming. `stop_at_track_end` is the hook
  `services::advance_on_track_end` asks before starting the next track.
- `schedule.rs` — pure functions: which night a moment belongs to
  (`window_opened_on`), the remaining-time, end-time and clock labels, the fade
  gain curve and the tick spacing. Tests live here.
- `settings.rs` — the "Sleep timer" group of Settings → General (automatic timer
  on/off, from / until, duration) and the fade-out switch shared with the Tools
  page. The group carries `SETTINGS_ANCHOR`: the Tools page's "Settings" button
  dispatches `settings_view::OpenSleepTimerSettings`, and `MainView` opens the
  General page scrolled to this group. `SleepTimerSettings` itself lives in
  `settings_store.rs`.

The Tools page is `tools/timer.rs`; the badge is `sleep_timer_badge` in
`main_view.rs`, drawn through `WindowTitleBar::center`.

## Behaviour

- **Wall-clock countdown.** A timer counts real time, not playback time: a
  manual pause does not stop it. When it runs out while nothing plays it just
  turns off.
- **Ticks only while armed.** The task wakes on whole-second boundaries of the
  remaining time (so the badge changes once a second), every `FADE_STEP` during
  the fade, and never when the timer is off or armed for end of track.
- **Fade-out** (on by default) lowers the output gain over the last `FADE`
  (30 s) with a squared curve (linear amplitude sounds like it drops only at the
  very end). It sets `Output` volume directly and never touches the saved
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
- **Countdown in the badge, clock time in Tools.** The title-bar badge shows
  the time left (a glance while falling asleep); the Tools status shows when it
  will pause ("Pauses at 23:47"), which is what one plans by. The end time is
  recomputed from the local clock on every relabel rather than stored, so a
  wall-clock change can't leave it stale; it drops the seconds, matching the
  fade start for whole-minute presets.
- The status line in Tools says when a timer was started automatically, so an
  unexpected pause is explainable.
