# secret_store

Seals the secrets the app keeps in its config (server passwords and tokens, scrobbling
sessions) so the file alone does not give them away. A value is sealed with
ChaCha20-Poly1305 under a data key. That key lives in the OS keychain where the platform
allows it, otherwise in a file wrapped by a key compiled into the binary. GPUI-free; knows
nothing of which fields are secret.

Not here: which settings fields are sealed and when the file is rewritten
(`pawse/src/doc.md`, "Settings persistence").

## Files

- `lib.rs` — `init`, the key set (`Keys`, `assemble`), `conceal` / `reveal`,
  `watch_reseal`, `Source`.
- `crypto.rs` — `Key` (id = first 4 bytes of its SHA-256, hex), `seal`, `open`,
  `is_sealed`, the format.
- `embedded.rs` — the binary key E, derived from a seed fixed in the source.
- `file_key.rs` — the file key `K_f` in `secret-<id of E>.key`, sealed with E.
- `keychain/` — `read` / `write` of the 32-byte data key: `windows.rs` (Credential
  Manager via `keyring-core`, target `data-key.pawse`), `linux.rs` (Secret Service or the
  Flatpak secret portal via `oo7`, label "Pawse", attributes `application=pawse`,
  `kind=data-key`). Compiled on those two platforms only.
- `serde_string.rs` — `#[serde(with)]` for a `String` field.
- `tests.rs` — sealing, key order, file key creation and its refusals, the pinned id of E.

## Spec

- Format: `pawse:v1:<key id>:<base64 (no pad) of nonce(12) ‖ ciphertext+tag>`, with a fresh
  nonce on every seal, so the text changes on every save.
- `init(dir)` runs once, blocking, before anything is (de)serialized. Keychain `Found` →
  `K_kc`. `NotFound` → generate and store a new one. An error (locked, denied, no
  service) → no `K_kc` this run and nothing is created: a new key would orphan every
  value sealed with the old one. An item that is not 32 bytes is left alone too.
- The file key is read when its file exists. It is created only when there is no `K_kc`.
  A file that does not open with this build's E is never overwritten: it belongs to a
  build with another E. The id of E in the name keeps such builds apart. It is written
  `create_new` with mode 0600 on unix and removed if the write fails.
- Sealing uses `K_kc`, else `K_f`, else E. Opening tries every key at hand whose id
  matches.
- `reveal`: a plain non-empty value is taken as is and flags a reseal. A sealed value
  that opens with a key other than the sealing one flags a reseal. One that does not open
  stays as the sealed text, and `conceal` writes it back unchanged. The user re-enters
  the secret, nothing is dropped. An empty value stays empty.
- The reseal flag is thread-local: `watch_reseal` resets it, runs the deserialization
  on the same thread, and reads it.
- Without `init` (tests), the key set is E only, with no I/O.
- E is the same in every build, so a local build on the real `HOME` reads the values the
  installed release sealed. A test pins its id: a new E opens no existing file key, and
  every fallback value would need re-entering.

## Decisions

Owner's calls; ask before changing.

- One data key in the keychain, values sealed in the config (the Chrome / Electron
  safeStorage shape), not one keychain item per secret.
- macOS uses no keychain. A self-signed build gets a `cdhash:` partition, so every
  update or rebuild would ask for the login password. That changes only with a Developer
  ID (`teamid:` partition).
- The fallback is a random file key wrapped by a public E, not one from a CI secret: a
  key compiled into a shipped binary can be read out of it anyway, and a secret one locks
  every other build out of the user's values. Not a key per app version either:
  per-version keys add no protection and break reading a newer file.
- E never changes. A new seed orphans every fallback file.
- Own thin wrappers rather than gpui's credentials API, which names items after Zed.
- The Flatpak manifest gets no `org.freedesktop.secrets` talk-name. Without a portal
  backend the file key is used.
