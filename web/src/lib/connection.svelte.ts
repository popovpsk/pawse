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
};

export type QueueItem = {
  id: number;
  title: string;
  artist: string | null;
  cover_id: number | null;
};

export type Status = "connecting" | "open" | "reconnecting";

type Cmd =
  | { cmd: "play_pause" }
  | { cmd: "next" }
  | { cmd: "prev" }
  | { cmd: "seek"; position_ms: number }
  | { cmd: "play_at"; index: number };

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

  coverUrl = $derived(this.coverId !== null ? `/api/cover?v=${this.coverId}` : null);

  #queueRev = -1;
  #ws: WebSocket | null = null;
  #backoff = 1000;
  #timer: ReturnType<typeof setTimeout> | null = null;

  #base = 0;
  #baseAt = 0;
  #raf: number | null = null;
  #seeking = false;

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
    this.status = this.#ws ? "reconnecting" : "connecting";
    const ws = new WebSocket(this.#url());
    this.#ws = ws;
    ws.onopen = () => {
      this.status = "open";
      this.#backoff = 1000;
    };
    ws.onmessage = (event) => {
      try {
        this.#apply(JSON.parse(event.data) as PlayerState);
      } catch {}
    };
    ws.onclose = () => this.#scheduleReconnect();
    ws.onerror = () => ws.close();
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
    fetch("/api/queue")
      .then((r) => r.json())
      .then((q: QueueItem[]) => {
        this.queue = q;
      })
      .catch(() => {});
  }

  coverUrlFor(coverId: number | null): string | null {
    return coverId !== null ? `/api/cover?id=${coverId}` : null;
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
