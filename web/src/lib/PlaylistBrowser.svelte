<script lang="ts">
  import { untrack } from "svelte";
  import { fade } from "svelte/transition";
  import {
    Remote,
    type PlaylistDetail,
    type PlaylistEntry,
    type PlaylistTrack,
  } from "./connection.svelte";
  import TrackRow from "./TrackRow.svelte";

  let {
    remote,
    inDetail = $bindable(false),
    detailName = $bindable(""),
  }: {
    remote: Remote;
    inDetail?: boolean;
    detailName?: string;
  } = $props();

  let playlists = $state<PlaylistEntry[] | null>(null);
  let detail = $state<PlaylistDetail | null>(null);
  let viewing = $state(false);
  let currentId = $state<number | null>(null);
  let pendingName = $state("");
  let loading = $state(false);
  let error = $state(false);
  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  let listEl = $state<HTMLElement | null>(null);
  let detailGen = 0;

  $effect(() => {
    remote.libraryRev;
    untrack(() => {
      loadPlaylists();
      if (viewing && currentId !== null) loadDetail(currentId);
    });
  });
  $effect(() => {
    inDetail = viewing;
  });
  $effect(() => {
    detailName = detail !== null ? detail.name : pendingName;
  });

  async function loadPlaylists() {
    error = false;
    try {
      const r = await fetch("/api/playlists");
      if (!r.ok) throw new Error();
      playlists = await r.json();
    } catch {
      if (!viewing) error = true;
    }
  }

  async function loadDetail(id: number) {
    const gen = ++detailGen;
    loading = true;
    error = false;
    try {
      const r = await fetch(`/api/playlist?id=${id}`);
      if (!r.ok) throw new Error();
      const fetched: PlaylistDetail = await r.json();
      if (gen !== detailGen) return;
      detail = fetched;
    } catch {
      if (gen === detailGen) error = true;
    } finally {
      if (gen === detailGen) loading = false;
    }
  }

  function openPlaylist(playlist: PlaylistEntry) {
    detail = null;
    currentId = playlist.id;
    pendingName = playlist.name;
    viewing = true;
    loadDetail(playlist.id);
    listEl?.scrollTo(0, 0);
  }

  function retry() {
    if (viewing && currentId !== null) loadDetail(currentId);
    else loadPlaylists();
  }

  export function goBack() {
    detailGen++;
    detail = null;
    viewing = false;
    loading = false;
    error = false;
    listEl?.scrollTo(0, 0);
    if (playlists === null || playlists.length === 0) loadPlaylists();
  }

  function playTrack(track: PlaylistTrack) {
    if (detail === null) return;
    remote.playPlaylistTrack(detail.id, track.id);
  }

  function queueTrack(track: PlaylistTrack) {
    if (detail === null) return;
    remote.queuePlaylistTrack(detail.id, track.id);
    showToast(`Queued: ${track.title}`);
  }

  function queueAll() {
    if (detail === null) return;
    remote.queuePlaylist(detail.id);
    showToast(`Queued: ${detail.name}`);
  }

  function showToast(message: string) {
    toast = message;
    if (toastTimer !== null) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast = null;
      toastTimer = null;
    }, 1800);
  }
</script>

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
      {#if playlists === null}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">Loading…</p>
      {:else if playlists.length === 0}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">No playlists</p>
      {:else}
        {#each playlists as playlist (playlist.id)}
          <button
            class="flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition active:scale-[0.99] hover:bg-white/5"
            onclick={() => openPlaylist(playlist)}
          >
            <span class="min-w-0 flex-1 truncate text-sm text-neutral-200">{playlist.name}</span>
            <span class="flex-shrink-0 text-xs text-neutral-500">{playlist.track_count}</span>
          </button>
        {/each}
      {/if}
    {:else if loading && detail === null}
      <p class="px-3 py-10 text-center text-sm text-neutral-500">Loading…</p>
    {:else if detail !== null}
      {#if detail.tracks.length > 0}
        <div class="flex items-center justify-end px-3 py-2">
          <button
            class="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white"
            aria-label="Add all to queue"
            onclick={queueAll}
          >
            <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
              <path d="M4 6h9M4 12h9M4 18h6" />
              <path d="M18 9v6M15 12h6" />
            </svg>
          </button>
        </div>
      {/if}
      {#if detail.tracks.length === 0}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">No tracks in this playlist</p>
      {/if}
      {#each detail.tracks as track (track.id)}
        <TrackRow {remote} {track} onplay={() => playTrack(track)} onqueue={() => queueTrack(track)} />
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
