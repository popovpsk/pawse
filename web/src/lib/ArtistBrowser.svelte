<script lang="ts">
  import { fade } from "svelte/transition";
  import {
    Remote,
    formatTime,
    type AlbumTrack,
    type ArtistAlbum,
    type ArtistDetail,
    type ArtistEntry,
  } from "./connection.svelte";

  let {
    remote,
    inDetail = $bindable(false),
    detailName = $bindable(""),
    detailHasPartial = $bindable(false),
    detailFull = $bindable(false),
  }: {
    remote: Remote;
    inDetail?: boolean;
    detailName?: string;
    detailHasPartial?: boolean;
    detailFull?: boolean;
  } = $props();

  let artists = $state<ArtistEntry[] | null>(null);
  let detail = $state<ArtistDetail | null>(null);
  let viewing = $state(false);
  let currentId = $state<number | null>(null);
  let pendingName = $state("");
  let full = $state(false);
  let loading = $state(false);
  let error = $state(false);
  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  let listEl = $state<HTMLElement | null>(null);
  let detailGen = 0;

  $effect(() => {
    loadArtists();
  });

  $effect(() => {
    inDetail = viewing;
  });
  $effect(() => {
    detailName = artistName(detail !== null ? detail.name : pendingName);
  });
  $effect(() => {
    detailHasPartial = detail?.has_partial ?? false;
  });
  $effect(() => {
    detailFull = full;
  });

  async function loadArtists() {
    error = false;
    try {
      const r = await fetch("/api/artists");
      if (!r.ok) throw new Error();
      artists = await r.json();
    } catch {
      if (!viewing) error = true;
    }
  }

  async function loadDetail(id: number, wantFull: boolean) {
    const gen = ++detailGen;
    loading = true;
    error = false;
    try {
      const r = await fetch(`/api/artist?id=${id}&full=${wantFull ? 1 : 0}`);
      if (!r.ok) throw new Error();
      const fetched: ArtistDetail = await r.json();
      if (gen !== detailGen) return;
      detail = fetched;
      full = wantFull;
    } catch {
      if (gen === detailGen) error = true;
    } finally {
      if (gen === detailGen) loading = false;
    }
  }

  function openArtist(artist: ArtistEntry) {
    detail = null;
    full = false;
    currentId = artist.id;
    pendingName = artist.name;
    viewing = true;
    loadDetail(artist.id, false);
    listEl?.scrollTo(0, 0);
  }

  function retry() {
    if (viewing && currentId !== null) loadDetail(currentId, full);
    else loadArtists();
  }

  export function toggleFull() {
    if (detail === null) return;
    loadDetail(detail.id, !full);
  }

  export function goBack() {
    detailGen++;
    detail = null;
    viewing = false;
    loading = false;
    error = false;
    listEl?.scrollTo(0, 0);
    if (artists === null || artists.length === 0) loadArtists();
  }

  function playTrack(track: AlbumTrack) {
    if (detail === null) return;
    remote.playArtistTrack(detail.id, track.id, full);
  }

  function queueAlbum(album: ArtistAlbum) {
    if (detail === null) return;
    remote.queueArtistAlbum(detail.id, album.album_id, full);
    showToast(`Queued: ${albumTitle(album)}`);
  }

  function queueTrack(track: AlbumTrack) {
    if (detail === null) return;
    remote.queueArtistTrack(detail.id, track.id, full);
    showToast(`Queued: ${track.title}`);
  }

  function showToast(message: string) {
    toast = message;
    if (toastTimer !== null) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast = null;
      toastTimer = null;
    }, 1800);
  }

  function artistName(name: string): string {
    return name || "No metadata";
  }

  function albumTitle(album: ArtistAlbum): string {
    return album.title || "No metadata";
  }

  type Row = { kind: "disc"; disc: number } | { kind: "track"; track: AlbumTrack };

  function albumRows(album: ArtistAlbum): Row[] {
    const multiDisc = album.tracks.some((t) => t.disc_number > 1);
    if (!multiDisc) return album.tracks.map((track) => ({ kind: "track", track }));
    const rows: Row[] = [];
    let disc = 0;
    for (const track of album.tracks) {
      if (track.disc_number !== disc) {
        disc = track.disc_number;
        rows.push({ kind: "disc", disc });
      }
      rows.push({ kind: "track", track });
    }
    return rows;
  }
</script>

{#snippet coverFallback()}
  <div class="flex h-full w-full items-center justify-center text-neutral-600">
    <svg class="h-1/2 w-1/2" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <path stroke-linecap="round" stroke-linejoin="round" d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </svg>
  </div>
{/snippet}

<div class="relative flex min-h-0 flex-1 flex-col">
  <div bind:this={listEl} class="min-h-0 flex-1 overflow-y-auto px-2 pb-[max(1rem,env(safe-area-inset-bottom))]">
    {#if error}
      <div class="flex flex-col items-center gap-3 px-3 py-10">
        <p class="text-center text-sm text-neutral-500">Failed to load. Check the connection.</p>
        <button
          class="rounded-full bg-white/10 px-4 py-1.5 text-sm text-neutral-200 transition active:scale-95 hover:bg-white/20"
          onclick={retry}
        >
          Retry
        </button>
      </div>
    {:else if !viewing}
      {#if artists === null}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">Loading…</p>
      {:else if artists.length === 0}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">No artists found</p>
      {:else}
        {#each artists as artist (artist.id)}
          <button
            class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-left transition active:scale-[0.99] hover:bg-white/5"
            onclick={() => openArtist(artist)}
          >
            <div class="h-11 w-11 flex-shrink-0 overflow-hidden rounded-full bg-neutral-800">
              {#if artist.cover_ids.length > 0}
                <img src={remote.coverUrlFor(artist.cover_ids[0])} alt="" loading="lazy" class="h-full w-full object-cover" />
              {:else}
                {@render coverFallback()}
              {/if}
            </div>
            <span class="min-w-0 flex-1 truncate text-sm text-neutral-200">{artistName(artist.name)}</span>
            <span class="flex-shrink-0 text-xs text-neutral-500">{artist.track_count}</span>
          </button>
        {/each}
      {/if}
    {:else if loading && detail === null}
      <p class="px-3 py-10 text-center text-sm text-neutral-500">Loading…</p>
    {:else if detail !== null}
      {#if detail.albums.length === 0}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">No tracks for this artist</p>
      {/if}
      {#each detail.albums as album, albumIx (albumIx)}
        <section class="mb-2">
          <div class="flex items-center gap-3 px-3 py-3">
            <div class="h-14 w-14 flex-shrink-0 overflow-hidden rounded-md bg-neutral-800">
              {#if album.cover_id !== null}
                <img src={remote.coverUrlFor(album.cover_id)} alt="" loading="lazy" class="h-full w-full object-cover" />
              {:else}
                {@render coverFallback()}
              {/if}
            </div>
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm font-semibold">{albumTitle(album)}</p>
              <p class="text-xs text-neutral-500">
                {album.year ?? ""}{album.year !== null && album.partial && !full ? " · " : ""}{album.partial && !full ? "partial" : ""}
              </p>
            </div>
            <button
              class="flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white"
              aria-label="Add album to queue"
              onclick={() => queueAlbum(album)}
            >
              <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                <path d="M4 6h9M4 12h9M4 18h6" />
                <path d="M18 9v6M15 12h6" />
              </svg>
            </button>
          </div>
          {#each albumRows(album) as row, rowIx (rowIx)}
            {#if row.kind === "disc"}
              <p class="px-3 pb-1 pt-3 text-xs font-semibold text-neutral-500">Disc {row.disc}</p>
            {:else}
              <div
                class={`group flex w-full items-center rounded-xl transition hover:bg-white/5 ${
                  row.track.id === remote.currentTrackId ? "bg-white/10" : ""
                }`}
              >
                <button
                  class="flex min-w-0 flex-1 items-center gap-3 py-2 pl-3 pr-2 text-left"
                  onclick={() => playTrack(row.track)}
                >
                  {#if row.track.id === remote.currentTrackId}
                    <svg class="h-4 w-4 flex-shrink-0 text-emerald-400" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M8 5v14l11-7z" />
                    </svg>
                  {:else}
                    <span class="w-4 flex-shrink-0 text-right text-xs tabular-nums text-neutral-500">
                      {row.track.track_number ?? "·"}
                    </span>
                  {/if}
                  <span
                    class={`min-w-0 flex-1 truncate text-sm ${
                      row.track.id === remote.currentTrackId ? "font-semibold text-white" : "text-neutral-200"
                    }`}
                  >
                    {row.track.title}
                  </span>
                  <span class="flex-shrink-0 text-xs tabular-nums text-neutral-500">
                    {formatTime(row.track.duration_ms)}
                  </span>
                </button>
                <button
                  class="mr-1 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:focus-visible:opacity-100 [@media(hover:hover)]:group-hover:opacity-100"
                  aria-label="Add to queue"
                  onclick={() => queueTrack(row.track)}
                >
                  <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                    <path d="M4 6h9M4 12h9M4 18h6" />
                    <path d="M18 9v6M15 12h6" />
                  </svg>
                </button>
              </div>
            {/if}
          {/each}
        </section>
      {/each}
    {/if}
  </div>

  {#if toast}
    <div
      class="pointer-events-none absolute inset-x-0 bottom-3 z-50 flex justify-center px-3"
      transition:fade={{ duration: 150 }}
    >
      <span class="max-w-full truncate rounded-full bg-emerald-400 px-4 py-2 text-sm font-medium text-neutral-950 shadow-lg">
        {toast}
      </span>
    </div>
  {/if}
</div>
