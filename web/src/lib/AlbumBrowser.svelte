<script lang="ts">
  import { untrack } from "svelte";
  import { fade } from "svelte/transition";
  import {
    Remote,
    type AlbumDetail,
    type AlbumEntry,
    type AlbumSort,
    type AlbumTrack,
  } from "./connection.svelte";
  import AlbumTracks from "./AlbumTracks.svelte";
  import SortBar from "./SortBar.svelte";
  import { setAlbumSort, sortPrefs } from "./sort.svelte";

  let {
    remote,
    inDetail = $bindable(false),
    detailName = $bindable(""),
  }: {
    remote: Remote;
    inDetail?: boolean;
    detailName?: string;
  } = $props();

  let albums = $state<AlbumEntry[] | null>(null);
  let detail = $state<AlbumDetail | null>(null);
  let viewing = $state(false);
  let currentId = $state<number | null>(null);
  let pendingName = $state("");
  let loading = $state(false);
  let error = $state(false);
  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  let listEl = $state<HTMLElement | null>(null);
  let detailGen = 0;
  let listGen = 0;

  const sortOptions: { value: AlbumSort; label: string; hints: [string, string] }[] = [
    { value: "artist", label: "Artist", hints: ["A–Z", "Z–A"] },
    { value: "title", label: "Title", hints: ["A–Z", "Z–A"] },
    { value: "year", label: "Year", hints: ["Oldest first", "Newest first"] },
  ];

  $effect(() => {
    remote.libraryRev;
    sortPrefs.albums.sort;
    sortPrefs.albums.desc;
    untrack(() => {
      refresh();
    });
  });
  $effect(() => {
    inDetail = viewing;
  });
  $effect(() => {
    detailName = albumTitle(detail !== null ? detail.title : pendingName);
  });

  async function refresh() {
    if (!(await loadAlbums())) return;
    if (!viewing || albums === null || currentId === null) return;
    const id = currentId;
    if (!albums.some((a) => a.id === id)) {
      goBack();
      return;
    }
    loadDetail(id);
  }

  async function loadAlbums(): Promise<boolean> {
    const gen = ++listGen;
    const { sort, desc } = sortPrefs.albums;
    error = false;
    try {
      const r = await fetch(`/api/albums?sort=${sort}&desc=${desc ? 1 : 0}`);
      if (!r.ok) throw new Error();
      const fetched: AlbumEntry[] = await r.json();
      if (gen !== listGen) return false;
      albums = fetched;
      return true;
    } catch {
      if (gen === listGen && !viewing) error = true;
      return false;
    }
  }

  async function loadDetail(id: number) {
    const gen = ++detailGen;
    loading = true;
    error = false;
    try {
      const r = await fetch(`/api/album?id=${id}`);
      if (!r.ok) throw new Error();
      const fetched: AlbumDetail = await r.json();
      if (gen !== detailGen) return;
      detail = fetched;
    } catch {
      if (gen === detailGen) error = true;
    } finally {
      if (gen === detailGen) loading = false;
    }
  }

  function changeSort(sort: AlbumSort, desc: boolean) {
    setAlbumSort(sort, desc);
    listEl?.scrollTo(0, 0);
  }

  function openAlbum(album: AlbumEntry) {
    detail = null;
    currentId = album.id;
    pendingName = album.title;
    viewing = true;
    loadDetail(album.id);
    listEl?.scrollTo(0, 0);
  }

  function retry() {
    if (viewing && currentId !== null) loadDetail(currentId);
    else loadAlbums();
  }

  export function goBack() {
    detailGen++;
    detail = null;
    viewing = false;
    loading = false;
    error = false;
    listEl?.scrollTo(0, 0);
    if (albums === null || albums.length === 0) loadAlbums();
  }

  function playTrack(track: AlbumTrack) {
    if (detail === null) return;
    remote.playAlbumTrack(detail.id, track.id);
  }

  function queueTrack(track: AlbumTrack) {
    if (detail === null) return;
    remote.queueAlbumTrack(detail.id, track.id);
    showToast(`Queued: ${track.title}`);
  }

  function queueAll() {
    if (detail === null) return;
    remote.queueAlbum(detail.id);
    showToast(`Queued: ${albumTitle(detail.title)}`);
  }

  function showToast(message: string) {
    toast = message;
    if (toastTimer !== null) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast = null;
      toastTimer = null;
    }, 1800);
  }

  function albumTitle(title: string): string {
    return title || "No metadata";
  }

  function subtitle(artist: string, year: number | null): string {
    return [artist, year ?? ""].filter((part) => part !== "").join(" · ");
  }
</script>

{#snippet cover(coverId: number | null)}
  {#if coverId !== null}
    <img src={remote.coverUrlFor(coverId)} alt="" loading="lazy" class="h-full w-full object-cover" />
  {:else}
    <div class="flex h-full w-full items-center justify-center text-neutral-600">
      <svg class="h-1/2 w-1/2" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="M9 18V5l12-2v13" />
        <circle cx="6" cy="18" r="3" />
        <circle cx="18" cy="16" r="3" />
      </svg>
    </div>
  {/if}
{/snippet}

<div class="relative flex min-h-0 flex-1 flex-col">
  {#if !viewing}
    <SortBar
      options={sortOptions}
      sort={sortPrefs.albums.sort}
      desc={sortPrefs.albums.desc}
      onchange={changeSort}
    />
  {/if}
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
      {#if albums === null}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">Loading…</p>
      {:else if albums.length === 0}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">No albums found</p>
      {:else}
        {#each albums as album (album.id)}
          <button
            class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-left transition active:scale-[0.99] hover:bg-white/5"
            onclick={() => openAlbum(album)}
          >
            <div class="h-11 w-11 flex-shrink-0 overflow-hidden rounded-md bg-neutral-800">
              {@render cover(album.cover_id)}
            </div>
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm text-neutral-200">{albumTitle(album.title)}</p>
              <p class="truncate text-xs text-neutral-500">{subtitle(album.artist, album.year)}</p>
            </div>
          </button>
        {/each}
      {/if}
    {:else if loading && detail === null}
      <p class="px-3 py-10 text-center text-sm text-neutral-500">Loading…</p>
    {:else if detail !== null}
      <div class="flex items-center gap-3 px-3 py-3">
        <div class="h-14 w-14 flex-shrink-0 overflow-hidden rounded-md bg-neutral-800">
          {@render cover(detail.cover_id)}
        </div>
        <div class="min-w-0 flex-1">
          <p class="truncate text-sm font-semibold">{albumTitle(detail.title)}</p>
          <p class="truncate text-xs text-neutral-500">{subtitle(detail.artist, detail.year)}</p>
        </div>
        {#if detail.tracks.length > 0}
          <button
            class="flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white"
            aria-label="Add album to queue"
            onclick={queueAll}
          >
            <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
              <path d="M4 6h9M4 12h9M4 18h6" />
              <path d="M18 9v6M15 12h6" />
            </svg>
          </button>
        {/if}
      </div>
      {#if detail.tracks.length === 0}
        <p class="px-3 py-10 text-center text-sm text-neutral-500">No tracks in this album</p>
      {/if}
      <AlbumTracks {remote} tracks={detail.tracks} onplay={playTrack} onqueue={queueTrack} />
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
