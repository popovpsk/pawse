# Native sample rate (Linux / PipeWire)

On Linux, Pawse's bit-perfect toggle is called **Native sample rate**. With it on,
every track plays at the rate it was recorded at: Pawse opens its PipeWire stream
at the source rate and asks PipeWire to run the graph — and therefore the DAC — at
that exact rate, so nothing in the chain resamples.

The ✓ indicator next to the toggle only lights up when the hardware really is at
the track's rate. Pawse reads that back from the kernel
(`/proc/asound/card*/pcm*p/sub*/hw_params`), it does not assume it.

## What it does *not* do

- It does **not** take the sound card away from the rest of the system. Other apps
  keep playing — they get resampled to *our* rate while Pawse is active, not the
  other way around.
- It does not change any of your configuration files. The rate is forced per
  stream (`node.force-rate` on Pawse's own PipeWire node) and disappears the
  moment Pawse's stream goes away, including if Pawse is killed.
- It does not touch volume. See [Volume](#volume) below.

Switching rates suspends and reopens the output device, so expect a short silence
between tracks with different rates. That is PipeWire reconfiguring the DAC, not a
glitch.

## Requirements

- A running PipeWire server. Without it the toggle is not shown at all.
- The `pipewire-alsa` package (it provides `/usr/share/alsa/alsa.conf.d/50-pipewire.conf`).
  Pawse opens the `pipewire` ALSA PCM directly; if that PCM does not exist, enabling
  the mode fails with a notification and playback stays in shared mode.
- A DAC that actually supports the track's rate.

## Checking that it works

While a track is playing:

```bash
# What the hardware is really doing (the ground truth):
cat /proc/asound/card*/pcm*p/sub*/hw_params

# What the graph is doing — the driver node's RATE is the device rate:
pw-top

# What the sound server thinks the sink is set to:
pactl list sinks | grep -A2 'Name:.*alsa_output'
```

Play a 44.1 kHz track: `hw_params` should say `rate: 44100 (44100/1)`. Switch to a
96 kHz track: it should say `rate: 96000`. If `hw_params` says `closed`, nothing is
playing on that card right now.

## When the rate does not switch

The interesting case is when *both* the track and the DAC support a rate, but the
sound server refuses to move the graph to it. Pawse shows a **Why?** button next to
the indicator when it detects this, and writes a diagnostic line (including your
current PipeWire clock settings) to its log file.

Things to check, in the order they usually bite:

### 1. `settings.check-rate` is on

By default PipeWire does not validate a forced rate against the allowed list. If
your distro (or you) turned `settings.check-rate = true` on, the rate must be in
`clock.allowed-rates`. Look at the current settings:

```bash
pw-metadata -n settings
```

Add the rates you care about — at runtime:

```bash
pw-metadata -n settings 0 clock.allowed-rates '[ 44100 48000 88200 96000 176400 192000 ]'
```

or permanently, in `~/.config/pipewire/pipewire.conf.d/10-rates.conf`:

```
context.properties = {
    default.clock.allowed-rates = [ 44100 48000 88200 96000 176400 192000 ]
}
```

Then restart the server: `systemctl --user restart pipewire pipewire-pulse wireplumber`.

### 2. The device rate is pinned in WirePlumber

A rule that sets `audio.rate` (or a narrow `audio.allowed-rates`) on the ALSA node
overrides everything Pawse asks for. Check
`~/.config/wireplumber/wireplumber.conf.d/*.conf` and your distro's files in
`/usr/share/wireplumber/` for `audio.rate`, and remove the pin (or widen it) for
the device you listen through.

### 3. Someone else is holding the graph

- Another client with `node.force-rate` (some tools set it permanently) wins if it
  activated after us — the *last* node to force a rate decides.
- A global force overrides every per-stream force:
  ```bash
  pw-metadata -n settings          # look for clock.force-rate
  pw-metadata -n settings 0 clock.force-rate 0   # release it
  ```

### 4. The device cannot do that rate on this profile

HDMI outputs, Bluetooth sinks and some onboard codecs are 48 kHz only, and a
device's UCM/ACP profile can expose fewer rates than the hardware supports. Try a
different profile in your system's sound settings, or accept resampling for that
output.

### 5. PipeWire is old

Per-stream rate forcing behaves as described above on modern PipeWire (1.0+).
Older builds may only switch the rate while the device is idle.

## Volume

Bit-perfect means unity gain end to end:

- Pawse's own volume slider is disabled while the mode is on — any digital
  attenuation we apply would break it.
- The sink's volume still applies. If the card has a hardware mixer, PipeWire uses
  it and the digital signal stays untouched; if it does not, the attenuation is
  done in software and the ✓ turns off with a "System volume not at unity" note.
  Set the sink back to 100% (`pavucontrol`, or
  `pactl set-sink-volume @DEFAULT_SINK@ 100%`) to get it back.
- A per-app volume for Pawse, remembered by WirePlumber from an earlier session,
  has the same effect. Check the Playback tab of `pavucontrol`.

## Turning it off

Click the toggle again. The forced rate goes away with the stream, the graph
returns to whatever your system default is, and nothing is left behind in your
configuration. The toggle itself can be hidden again in Settings → Native sample
rate button.
