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
	$(FIXTURES_DIR)/1khz_16_44_1.wav

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

clean:
	rm -f $(FIXTURES)
