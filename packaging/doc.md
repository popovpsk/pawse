# packaging

Distribution channels that live outside `cargo packager` and the GitHub release.
Everything here is consumed by `.github/workflows/release.yml`.

## `aur/`

`PKGBUILD.tmpl` is the source for the `pawse-bin` AUR package. `.github/workflows/aur.yml`
downloads both `*-pacman.tar.gz` release assets, substitutes `__VERSION__` and the two
`__SHA256_*__` placeholders, uploads the rendered `PKGBUILD` as a workflow artifact,
and pushes to AUR only when the `AUR_SSH_PRIVATE_KEY` secret is set — so it is safe to
run before an AUR account exists.

It is a **separate workflow triggered by `release: published`**, not a job in
`release.yml`, because `release.yml` publishes drafts. A draft's assets are not served
from `releases/download/<tag>/`, so rendering the PKGBUILD inside the release run would
404 — and pushing a PKGBUILD whose `source` URLs are not live yet would break `makepkg`
for anyone who installed in that window. Publishing the draft is what fires this.

The tarballs come from cargo-packager's `pacman` format, which emits a `.tar.gz` plus
its own PKGBUILD; only the tarball is published, under a name the workflow picks
(`pawse_<version>_<arch>-pacman.tar.gz`) rather than whatever the packager chose, so
the template can rely on it.

**Unverified:** the tarball's internal layout. `package()` assumes a `usr/` tree and
fails loudly with a listing if that assumption is wrong — check the first release run
before pointing anyone at the AUR package.

## `flatpak/`

Manifest, AppStream metainfo and desktop entry for Flathub. The app id is
`io.github.popovpsk.pawse`, **not** the `dev.pawse.app` used for the macOS bundle:
Flathub requires the id's domain to be one you control and reachable over HTTPS, and
`pawse.dev` currently has NS records but no A record. The `io.github.` form resolves to
`github.com/popovpsk/pawse`, which is an exact match. The two ids are independent
namespaces and neither should be changed to follow the other.

Metainfo notes that cost time to rediscover: screenshot links must point at a tag or
commit, never a branch; a `releases` tag is mandatory and its version must be bumped
per release; release dates must not be in the future.

### Offline sources

Flathub builds with no network, so both dependency trees must be vendored into the
manifest before submitting, and regenerated for every release:

```sh
flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
flatpak-node-generator npm web/package-lock.json -o packaging/flatpak/node-sources.json
```

Both come from `flatpak/flatpak-builder-tools`. The generated files are not committed
yet — the manifest references them, so a build fails until they are produced.

### Status: drafted, never built

Nothing here has been through `flatpak-builder`; it was written on macOS. Two things
must be settled on a Linux machine before a Flathub PR:

1. **PipeWire.** The native-sample-rate path (`crates/audio_output/src/exclusive/linux`)
   opens the ALSA PCM named `pipewire`, which comes from the host's `pipewire-alsa`
   package and is absent from the Flatpak runtime. Confirm it either works through the
   proxied `xdg-run/pipewire-0` socket or degrades cleanly to the cpal shared path. If
   it hard-fails, the manifest needs a `pipewire` module. Check the bit-perfect
   indicator, not just that sound comes out.
2. **Last.fm and Discord.** Flathub builds from source on its own infrastructure, with
   no access to `LASTFM_API_KEY`, `LASTFM_API_SECRET` or `DISCORD_CLIENT_ID`. Both
   integrations self-disable without keys, so a Flathub build ships without scrobbling
   and without Discord presence unless users can supply their own keys.

`--filesystem=home` is there because the tag editor writes to the user's files;
reviewers ask about it, so the submission PR should say so up front.
