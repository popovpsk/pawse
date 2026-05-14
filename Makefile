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
