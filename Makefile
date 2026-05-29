FFMPEG_VERSION ?= 7.1
FFMPEG_URL ?= https://evermeet.cx/ffmpeg/ffmpeg-$(FFMPEG_VERSION).zip
BIN_DIR = bin
FFMPEG = $(BIN_DIR)/ffmpeg
FIXTURES_DIR = fixtures

FIXTURES = \
	$(FIXTURES_DIR)/sine_440_16_44_mono.wav \
	$(FIXTURES_DIR)/sine_440_16_48_mono.wav \
	$(FIXTURES_DIR)/sine_440_16_96_mono.wav \
	$(FIXTURES_DIR)/sine_440_24_44_mono.wav \
	$(FIXTURES_DIR)/sine_440_32_44_mono.wav \
	$(FIXTURES_DIR)/sine_440_16_44_stereo.wav \
	$(FIXTURES_DIR)/silence_16_44_mono.wav \
	$(FIXTURES_DIR)/1khz_16_44_1.wav \
	$(FIXTURES_DIR)/cover_front.png \
	$(FIXTURES_DIR)/tagged_basic.flac \
	$(FIXTURES_DIR)/tagged_track_disc_slash.flac \
	$(FIXTURES_DIR)/tagged_with_cover.flac \
	$(FIXTURES_DIR)/tagless.flac \
	$(FIXTURES_DIR)/tagged_mp3.mp3 \
	$(FIXTURES_DIR)/tagged_ogg.ogg

.PHONY: all generate clean bin-deps

all: generate

bin-deps: $(FFMPEG)

$(FFMPEG):
	@mkdir -p $(BIN_DIR)
	curl -L -o $(BIN_DIR)/ffmpeg.zip "$(FFMPEG_URL)"
	unzip -o $(BIN_DIR)/ffmpeg.zip -d $(BIN_DIR)
	rm $(BIN_DIR)/ffmpeg.zip
	chmod +x $(FFMPEG)

generate: bin-deps $(FIXTURES)

$(FIXTURES_DIR):
	@mkdir -p $(FIXTURES_DIR)

$(FIXTURES_DIR)/sine_440_16_44_mono.wav: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-acodec pcm_s16le -ar 44100 -ac 1 "$@"

$(FIXTURES_DIR)/sine_440_16_48_mono.wav: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-acodec pcm_s16le -ar 48000 -ac 1 "$@"

$(FIXTURES_DIR)/sine_440_16_96_mono.wav: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-acodec pcm_s16le -ar 96000 -ac 1 "$@"

$(FIXTURES_DIR)/sine_440_24_44_mono.wav: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-acodec pcm_s24le -ar 44100 -ac 1 "$@"

$(FIXTURES_DIR)/sine_440_32_44_mono.wav: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-acodec pcm_s32le -ar 44100 -ac 1 "$@"

$(FIXTURES_DIR)/sine_440_16_44_stereo.wav: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-acodec pcm_s16le -ar 44100 -ac 2 "$@"

$(FIXTURES_DIR)/silence_16_44_mono.wav: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "anullsrc=r=44100:cl=mono" \
		-acodec pcm_s16le -ar 44100 -ac 1 -t 0.5 "$@"

$(FIXTURES_DIR)/1khz_16_44_1.wav: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=1000:duration=2.0" \
		-acodec pcm_s16le -ar 44100 -ac 1 "$@"

# --- Cover art images ---

$(FIXTURES_DIR)/cover_front.png: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "color=red:1x1:d=0.1" -frames:v 1 -update 1 "$@"

# --- Tagged FLAC with metadata ---

$(FIXTURES_DIR)/tagged_basic.flac: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-metadata title="Test Track" \
		-metadata artist="Test Artist" \
		-metadata ALBUMARTIST="Test Album Artist" \
		-metadata album="Test Album" \
		-metadata track="3" \
		-metadata disc="1" \
		-metadata YEAR="2024" \
		-codec:a flac "$@"

$(FIXTURES_DIR)/tagged_track_disc_slash.flac: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-metadata title="Slash Track" \
		-metadata artist="Slash Artist" \
		-metadata ALBUMARTIST="Slash Album Artist" \
		-metadata album="Slash Album" \
		-metadata track="5/12" \
		-metadata disc="2/3" \
		-metadata YEAR="2023-06-15" \
		-codec:a flac "$@"

# --- FLAC with embedded cover art (CoverFront) ---

$(FIXTURES_DIR)/tagged_with_cover.flac: $(FIXTURES_DIR)/cover_front.png | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-metadata title="Cover Track" \
		-metadata artist="Cover Artist" \
		-metadata ALBUMARTIST="Cover Album Artist" \
		-metadata album="Cover Album" \
		-metadata track="1" \
		-metadata disc="1" \
		-metadata YEAR="2022" \
		-codec:a flac $(FIXTURES_DIR)/.tagged_with_cover_tmp.flac && \
	$(FFMPEG) -y -i $(FIXTURES_DIR)/.tagged_with_cover_tmp.flac \
		-i $(FIXTURES_DIR)/cover_front.png \
		-map 0 -map 1 -c copy -disposition:v:0 attached_pic "$@" && \
	rm -f $(FIXTURES_DIR)/.tagged_with_cover_tmp.flac

# --- Tagless FLAC (no metadata) ---

$(FIXTURES_DIR)/tagless.flac: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" -codec:a flac "$@"

# --- Tagged MP3 ---

$(FIXTURES_DIR)/tagged_mp3.mp3: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-metadata title="MP3 Track" \
		-metadata artist="MP3 Artist" \
		-metadata album="MP3 Album" \
		-metadata track="7" \
		-codec:a libmp3lame "$@"

# --- Tagged OGG ---

$(FIXTURES_DIR)/tagged_ogg.ogg: | $(FIXTURES_DIR)
	$(FFMPEG) -y -f lavfi -i "sine=frequency=440:duration=0.5" \
		-metadata title="OGG Track" \
		-metadata artist="OGG Artist" \
		-metadata album="OGG Album" \
		-metadata track="9" \
		-codec:a libvorbis "$@"

clean:
	rm -f $(FIXTURES)

# ============================================================
# Test orchestration
# ============================================================
#
# Inner loop:   make test           (seconds)
# Pre-push:     make test-san       (5-15 min, runs sanitizers + miri + careful)
# Pre-release:  make test-full      (test-san + leaks(1) on every test binary)
#
# Required tooling (one-time setup):
#   rustup toolchain install nightly
#   rustup +nightly component add rust-src miri
#   cargo install cargo-careful
#
# Notes:
#   - Miri does not support FFI, so only crates listed in MIRI_CRATES are run.
#     Add crates to MIRI_CRATES as you confirm they have no FFI in their tests.
#   - ASAN and TSAN are mutually exclusive (separate builds, separate processes).
#   - Sanitizer builds rebuild std (-Zbuild-std), so each is slow on first run.
#   - Nightly-based targets (careful/asan/tsan/miri) exclude the GUI crates
#     (pawse, ui_components, audio_engine) because gpui pulls in pathfinder_simd,
#     which fails to compile on current nightly. The audio/library crates listed
#     in SAN_CRATES are where unsafe and concurrency live, so that's what we
#     actually want to sanitize.

# test-leaks uses bash features (pipefail, process substitution, $'\t').
SHELL := /bin/bash

TARGET ?= $(shell rustc -vV | sed -n 's/host: //p')

SAN_CRATES = \
	-p audio_common \
	-p audio_decoder \
	-p audio_engine \
	-p audio_output \
	-p cue_parser \
	-p media_integration \
	-p music_indexer \
	-p music_library \
	-p pawse \
	-p ui_components \
	-p ui_resources

.PHONY: test test-careful test-asan test-tsan test-miri test-leaks test-san test-full help-test

test:
	cargo test --workspace

test-careful:
	cargo +nightly careful test $(SAN_CRATES)

test-asan:
	RUSTFLAGS="-Zsanitizer=address" \
	RUSTDOCFLAGS="-Zsanitizer=address" \
	cargo +nightly test $(SAN_CRATES) --target $(TARGET) -Zbuild-std

test-tsan:
	RUSTFLAGS="-Zsanitizer=thread" \
	RUSTDOCFLAGS="-Zsanitizer=thread" \
	cargo +nightly test $(SAN_CRATES) --target $(TARGET) -Zbuild-std

test-miri:
	cargo +nightly miri test $(SAN_CRATES)

# Runs every workspace test binary under leaks(1). Requires `jq` and `leaks`.
#
# The binaries are launched directly (not via `cargo test`), so cargo does not
# inject CARGO_MANIFEST_DIR. Tests that resolve fixtures via
# `std::env::var("CARGO_MANIFEST_DIR")` would panic; we set it per-binary from
# the artifact's manifest_path, exactly as cargo would.
#
# `leaks --atExit` exits 0 even when the wrapped binary's tests FAIL, so a
# failing binary is detected via its `test result: FAILED` line as well as via
# leaks' own non-zero exit (real leaks). Any failure makes the target exit 1.
test-leaks:
	@command -v jq    >/dev/null 2>&1 || { echo "jq is required for test-leaks"; exit 1; }
	@command -v leaks >/dev/null 2>&1 || { echo "leaks(1) is required for test-leaks"; exit 1; }
	@set -uo pipefail; \
	json=$$(mktemp); \
	if ! cargo test --workspace --no-run --message-format=json > "$$json"; then \
		rm -f "$$json"; echo "test-leaks: build failed"; exit 1; \
	fi; \
	status=0; \
	while IFS=$$'\t' read -r bin manifest; do \
		[ -n "$$bin" ] || continue; \
		echo "===> leaks: $$bin"; \
		dir=$$(dirname "$$manifest"); \
		out=$$(mktemp); \
		if MallocStackLogging=1 CARGO_MANIFEST_DIR="$$dir" \
			leaks --atExit -- "$$bin" 2>&1 | tee "$$out"; then lk=0; else lk=1; fi; \
		if [ "$$lk" -ne 0 ] || grep -q "test result: FAILED" "$$out"; then \
			echo "===> FAILURE: $$bin"; status=1; \
		fi; \
		rm -f "$$out"; \
	done < <(jq -r 'select(.executable != null and .profile.test == true) | "\(.executable)\t\(.manifest_path)"' "$$json"); \
	rm -f "$$json"; \
	[ "$$status" -eq 0 ] || { echo "test-leaks: failures detected"; exit 1; }

test-san: test-careful test-asan test-tsan test-miri

test-full: test-san test-leaks

help-test:
	@echo "Test targets:"
	@echo "  make test          - fast cargo test (inner loop)"
	@echo "  make test-careful  - cargo-careful, extra UB checks in std"
	@echo "  make test-asan     - AddressSanitizer (use-after-free, double-free, OOB)"
	@echo "  make test-tsan     - ThreadSanitizer (data races)"
	@echo "  make test-miri     - Miri (pure-Rust crates only — no FFI)"
	@echo "  make test-leaks    - macOS leaks(1) on each test binary"
	@echo "  make test-san      - careful + asan + tsan + miri (pre-push)"
	@echo "  make test-full     - test-san + test-leaks (pre-release)"
