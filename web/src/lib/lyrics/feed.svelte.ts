import { untrack } from "svelte";
import type { Remote, TrackLyrics } from "../connection.svelte";
import { parseLrc, type Lyrics } from "./parse";

export type LyricsStatus = "idle" | "loading" | "ready" | "searching" | "none" | "error";

const SEARCH_WAIT_MS = 35_000;

export class LyricsFeed {
  status = $state<LyricsStatus>("idle");
  lyrics = $state.raw<Lyrics | null>(null);

  #remote: Remote;
  #retries = $state(0);
  #key = "";
  #seq = 0;
  #trackId: number | null = null;
  #askedTrack: number | null = null;
  #connection = 0;
  #text: string | null = null;
  #searchTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(remote: Remote, wanted: () => boolean) {
    this.#remote = remote;
    $effect(() => {
      const connection = remote.connection;
      if (connection === 0 || !wanted()) return;
      const trackId = remote.trackId;
      const key = `${connection}:${trackId}:${remote.lyricsRev}:${this.#retries}`;
      untrack(() => this.#load(connection, trackId, key));
    });
  }

  retry() {
    this.#retries += 1;
  }

  #load(connection: number, trackId: number | null, key: string) {
    if (connection !== this.#connection) {
      this.#connection = connection;
      this.#askedTrack = null;
    }
    if (key === this.#key) return;
    this.#key = key;
    const seq = ++this.#seq;
    if (trackId === null) {
      this.#trackId = null;
      this.#text = null;
      this.lyrics = null;
      this.status = "idle";
      return;
    }
    if (trackId !== this.#trackId) {
      this.#trackId = trackId;
      this.#text = null;
      this.lyrics = null;
      this.status = "loading";
    } else if (this.status === "error") {
      this.status = "loading";
    }
    fetch(`/api/lyrics?id=${trackId}`)
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json() as Promise<TrackLyrics>;
      })
      .then((body) => {
        if (seq !== this.#seq) return;
        if (body.text !== this.#text) {
          this.#text = body.text;
          const parsed = body.text !== null ? parseLrc(body.text) : null;
          this.lyrics = parsed !== null && parsed.lines.length > 0 ? parsed : null;
        }
        let asked = false;
        if (!body.searchable && this.#askedTrack === trackId) this.#askedTrack = null;
        if (body.searchable && this.#askedTrack !== trackId) {
          this.#askedTrack = trackId;
          this.#remote.findLyrics(trackId);
          asked = true;
        }
        const searching = body.searching || asked;
        this.status = this.lyrics !== null ? "ready" : searching ? "searching" : "none";
        if (this.status === "searching") this.#waitForSearch(trackId);
      })
      .catch(() => {
        if (seq !== this.#seq) return;
        this.#key = "";
        if (this.lyrics === null) this.status = "error";
      });
  }

  #waitForSearch(trackId: number) {
    if (this.#searchTimer !== null) clearTimeout(this.#searchTimer);
    this.#searchTimer = setTimeout(() => {
      this.#searchTimer = null;
      if (this.#trackId !== trackId || this.status !== "searching") return;
      this.#retries += 1;
    }, SEARCH_WAIT_MS);
  }
}
