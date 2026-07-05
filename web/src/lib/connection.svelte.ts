export type RepeatMode = "off" | "all" | "one";

export type PlayerState = {
  v: number;
  has_track: boolean;
  title: string | null;
  artist: string | null;
  album: string | null;
  playing: boolean;
  position_ms: number;
  duration_ms: number;
  cover_id: number | null;
  queue_index: number | null;
  queue_rev: number;
  shuffle: boolean;
  repeat: RepeatMode;
  volume: number;
  volume_locked: boolean;
};

export type QueueItem = {
  id: number;
  title: string;
  artist: string | null;
  cover_id: number | null;
};

export type ArtistEntry = {
  id: number;
  name: string;
  track_count: number;
  cover_ids: number[];
};

export type AlbumTrack = {
  id: number;
  title: string;
  track_number: number | null;
  disc_number: number;
  duration_ms: number;
};

export type ArtistAlbum = {
  album_id: number | null;
  title: string;
  year: number | null;
  cover_id: number | null;
  partial: boolean;
  tracks: AlbumTrack[];
};

export type ArtistDetail = {
  id: number;
  name: string;
  has_partial: boolean;
  albums: ArtistAlbum[];
};

export type Status = "connecting" | "open" | "reconnecting";

type Cmd =
  | { cmd: "play_pause" }
  | { cmd: "next" }
  | { cmd: "prev" }
  | { cmd: "seek"; position_ms: number }
  | { cmd: "play_at"; index: number }
  | { cmd: "set_shuffle"; on: boolean }
  | { cmd: "set_repeat"; mode: RepeatMode }
  | { cmd: "set_volume"; volume: number }
  | { cmd: "play_artist_track"; artist_id: number; track_id: number; full: boolean }
  | { cmd: "queue_artist_track"; artist_id: number; track_id: number; full: boolean }
  | { cmd: "queue_artist_album"; artist_id: number; album_id: number | null; full: boolean };

export class Remote {
  status = $state<Status>("connecting");
  hasTrack = $state(false);
  title = $state<string | null>(null);
  artist = $state<string | null>(null);
  album = $state<string | null>(null);
  playing = $state(false);
  durationMs = $state(0);
  positionMs = $state(0);
  coverId = $state<number | null>(null);
  queueIndex = $state<number | null>(null);
  queue = $state<QueueItem[]>([]);
  shuffle = $state(false);
  repeat = $state<RepeatMode>("off");
  volume = $state(1);
  volumeLocked = $state(false);

  coverUrl = $derived(this.coverId !== null ? `/api/cover?id=${this.coverId}&size=full` : null);
  currentTrackId = $derived(
    this.queueIndex !== null ? (this.queue[this.queueIndex]?.id ?? null) : null,
  );

  #queueRev = -1;
  #queueFetchSeq = 0;
  #ws: WebSocket | null = null;
  #backoff = 1000;
  #timer: ReturnType<typeof setTimeout> | null = null;

  #base = 0;
  #baseAt = 0;
  #raf: number | null = null;
  #seeking = false;

  #volumeAdjusting = false;
  #volumeTimer: ReturnType<typeof setTimeout> | null = null;
  #volumePending: number | null = null;
  #volumeResetTimer: ReturnType<typeof setTimeout> | null = null;
  #volumeReleased = false;

  constructor() {
    this.#connect();
    addEventListener("online", () => this.#reconnectNow());
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") this.#reconnectNow();
    });
  }

  #url(): string {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    return `${proto}://${location.host}/ws`;
  }

  #connect() {
    const previous = this.#ws;
    if (previous) {
      previous.onopen = null;
      previous.onmessage = null;
      previous.onclose = null;
      previous.onerror = null;
      try {
        previous.close();
      } catch {}
    }
    this.status = previous ? "reconnecting" : "connecting";
    const ws = new WebSocket(this.#url());
    this.#ws = ws;
    ws.onopen = () => {
      if (this.#ws !== ws) return;
      this.status = "open";
      this.#backoff = 1000;
    };
    ws.onmessage = (event) => {
      if (this.#ws !== ws) return;
      try {
        this.#apply(JSON.parse(event.data) as PlayerState);
      } catch {}
    };
    ws.onclose = () => {
      if (this.#ws !== ws) return;
      this.#scheduleReconnect();
    };
    ws.onerror = () => {
      if (this.#ws !== ws) return;
      ws.close();
    };
  }

  #apply(state: PlayerState) {
    this.hasTrack = state.has_track;
    this.title = state.title;
    this.artist = state.artist;
    this.album = state.album;
    this.playing = state.playing;
    this.durationMs = state.duration_ms;
    this.coverId = state.cover_id;
    this.queueIndex = state.queue_index;
    this.shuffle = state.shuffle;
    this.repeat = state.repeat;
    this.volumeLocked = state.volume_locked;
    if (!this.#volumeAdjusting) this.volume = state.volume;
    this.#base = state.position_ms;
    this.#baseAt = performance.now();
    if (!this.#seeking) this.positionMs = state.position_ms;
    if (state.queue_rev !== this.#queueRev) {
      this.#queueRev = state.queue_rev;
      this.#fetchQueue();
    }
    this.#ensureTick();
  }

  #fetchQueue() {
    const seq = ++this.#queueFetchSeq;
    fetch("/api/queue")
      .then((r) => r.json())
      .then((q: QueueItem[]) => {
        if (seq === this.#queueFetchSeq) this.queue = q;
      })
      .catch(() => {});
  }

  coverUrlFor(coverId: number | null): string | null {
    return coverId !== null ? `/api/cover?id=${coverId}&size=small` : null;
  }

  playAt(index: number) {
    this.queueIndex = index;
    this.#send({ cmd: "play_at", index });
  }

  #ensureTick() {
    if (this.#raf !== null || !this.playing) return;
    const step = () => {
      this.#raf = null;
      if (this.playing && !this.#seeking) {
        const elapsed = performance.now() - this.#baseAt;
        const next = this.#base + elapsed;
        this.positionMs = this.durationMs > 0 ? Math.min(next, this.durationMs) : next;
      }
      if (this.playing) this.#raf = requestAnimationFrame(step);
    };
    this.#raf = requestAnimationFrame(step);
  }

  beginSeek() {
    this.#seeking = true;
  }

  previewSeek(ms: number) {
    this.#seeking = true;
    this.positionMs = ms;
  }

  endSeek(ms: number) {
    if (!this.#seeking) return;
    this.#seeking = false;
    this.#base = ms;
    this.#baseAt = performance.now();
    this.positionMs = ms;
    this.#send({ cmd: "seek", position_ms: Math.round(ms) });
    this.#ensureTick();
  }

  playPause() {
    this.playing = !this.playing;
    this.#base = this.positionMs;
    this.#baseAt = performance.now();
    this.#ensureTick();
    this.#send({ cmd: "play_pause" });
  }

  next() {
    this.#send({ cmd: "next" });
  }

  prev() {
    this.#send({ cmd: "prev" });
  }

  toggleShuffle() {
    this.shuffle = !this.shuffle;
    this.#send({ cmd: "set_shuffle", on: this.shuffle });
  }

  cycleRepeat() {
    const next: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };
    this.repeat = next[this.repeat];
    this.#send({ cmd: "set_repeat", mode: this.repeat });
  }

  previewVolume(value: number) {
    if (this.#volumeResetTimer !== null) {
      clearTimeout(this.#volumeResetTimer);
      this.#volumeResetTimer = null;
    }
    this.#volumeAdjusting = true;
    this.#volumeReleased = false;
    this.volume = value;
    this.#sendVolumeThrottled(value);
  }

  endVolume(value: number) {
    if (this.#volumeReleased) return;
    this.#volumeReleased = true;
    this.volume = value;
    this.#volumePending = null;
    if (this.#volumeTimer !== null) {
      clearTimeout(this.#volumeTimer);
      this.#volumeTimer = null;
    }
    this.#send({ cmd: "set_volume", volume: value });
    if (this.#volumeResetTimer !== null) clearTimeout(this.#volumeResetTimer);
    this.#volumeResetTimer = setTimeout(() => {
      this.#volumeResetTimer = null;
      this.#volumeAdjusting = false;
    }, 300);
  }

  #sendVolumeThrottled(value: number) {
    if (this.#volumeTimer !== null) {
      this.#volumePending = value;
      return;
    }
    this.#send({ cmd: "set_volume", volume: value });
    this.#volumeTimer = setTimeout(() => {
      this.#volumeTimer = null;
      if (this.#volumePending !== null) {
        const pending = this.#volumePending;
        this.#volumePending = null;
        this.#sendVolumeThrottled(pending);
      }
    }, 120);
  }

  playArtistTrack(artistId: number, trackId: number, full: boolean) {
    this.#send({ cmd: "play_artist_track", artist_id: artistId, track_id: trackId, full });
  }

  queueArtistTrack(artistId: number, trackId: number, full: boolean) {
    this.#send({ cmd: "queue_artist_track", artist_id: artistId, track_id: trackId, full });
  }

  queueArtistAlbum(artistId: number, albumId: number | null, full: boolean) {
    this.#send({ cmd: "queue_artist_album", artist_id: artistId, album_id: albumId, full });
  }

  #send(cmd: Cmd) {
    fetch("/api/command", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(cmd),
    }).catch(() => {});
  }

  #scheduleReconnect() {
    this.status = "reconnecting";
    const wait = Math.min(this.#backoff, 30000) * (0.5 + Math.random());
    this.#backoff *= 2;
    this.#clearTimer();
    this.#timer = setTimeout(() => this.#connect(), wait);
  }

  #reconnectNow() {
    const state = this.#ws?.readyState;
    if (state === WebSocket.CONNECTING || state === WebSocket.OPEN) return;
    this.#clearTimer();
    this.#backoff = 1000;
    this.#connect();
  }

  #clearTimer() {
    if (this.#timer !== null) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
  }
}

export function formatTime(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) ms = 0;
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
