export type PlayerState = {
  v: number;
  title: string | null;
  playing: boolean;
};

export type Status = "connecting" | "open" | "reconnecting";

export class Remote {
  status = $state<Status>("connecting");
  title = $state<string | null>(null);
  playing = $state(false);

  #ws: WebSocket | null = null;
  #backoff = 1000;
  #timer: ReturnType<typeof setTimeout> | null = null;

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
        const state = JSON.parse(event.data) as PlayerState;
        this.title = state.title;
        this.playing = state.playing;
      } catch {}
    };
    ws.onclose = () => this.#scheduleReconnect();
    ws.onerror = () => ws.close();
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
