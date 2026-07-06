<script lang="ts">
  import { fade } from "svelte/transition";
  import { Remote } from "./connection.svelte";
  import ArtistBrowser from "./ArtistBrowser.svelte";
  import PlaylistBrowser from "./PlaylistBrowser.svelte";
  import LikedBrowser from "./LikedBrowser.svelte";

  let { remote, onclose }: { remote: Remote; onclose: () => void } = $props();

  let tab = $state<"artists" | "playlists" | "liked">("artists");

  let artistBrowser = $state<ArtistBrowser | null>(null);
  let aInDetail = $state(false);
  let aName = $state("");
  let aHasPartial = $state(false);
  let aFull = $state(false);

  let plBrowser = $state<PlaylistBrowser | null>(null);
  let plInDetail = $state(false);
  let plName = $state("");

  const activeInDetail = $derived(
    tab === "artists" ? aInDetail : tab === "playlists" ? plInDetail : false,
  );

  const title = $derived(
    tab === "artists"
      ? aInDetail
        ? aName
        : "Artists"
      : tab === "playlists"
        ? plInDetail
          ? plName
          : "Playlists"
        : "Liked",
  );

  function goBack() {
    if (tab === "artists") artistBrowser?.goBack();
    else if (tab === "playlists") plBrowser?.goBack();
  }
</script>

<div
  class="absolute inset-0 z-40 flex flex-col bg-neutral-950 text-neutral-100"
  transition:fade={{ duration: 150 }}
>
  <header class="flex items-center gap-3 border-b border-white/10 px-4 py-3">
    {#if activeInDetail}
      <button
        class="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-full text-neutral-300 transition active:scale-90 hover:bg-white/10"
        aria-label="Back"
        onclick={goBack}
      >
        <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M15 18l-6-6 6-6" />
        </svg>
      </button>
    {/if}
    <h2 class="min-w-0 flex-1 truncate text-base font-semibold">{title}</h2>
    {#if tab === "artists" && aInDetail && aHasPartial}
      <button
        class={`flex h-10 flex-shrink-0 items-center gap-2 rounded-full px-4 text-xs font-medium transition active:scale-95 ${
          aFull ? "bg-emerald-400 text-neutral-950" : "bg-white/10 text-neutral-300"
        }`}
        onclick={() => artistBrowser?.toggleFull()}
      >
        <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="9" />
          <circle cx="12" cy="12" r="3" />
        </svg>
        Full albums
      </button>
    {/if}
    <button
      class="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white"
      aria-label="Close"
      onclick={onclose}
    >
      <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <path d="M6 6l12 12M18 6L6 18" />
      </svg>
    </button>
  </header>

  {#if !activeInDetail}
    <div class="flex items-center gap-1 border-b border-white/10 px-3 py-2">
      <button
        class={`flex-shrink-0 rounded-full px-3 py-1.5 text-sm font-semibold tracking-wide transition ${
          tab === "artists" ? "bg-white/10 text-white" : "text-neutral-400 hover:text-neutral-200"
        }`}
        onclick={() => (tab = "artists")}
      >
        Artists
      </button>
      <button
        class={`flex-shrink-0 rounded-full px-3 py-1.5 text-sm font-semibold tracking-wide transition ${
          tab === "playlists" ? "bg-white/10 text-white" : "text-neutral-400 hover:text-neutral-200"
        }`}
        onclick={() => (tab = "playlists")}
      >
        Playlists
      </button>
      <button
        class={`flex-shrink-0 rounded-full px-3 py-1.5 text-sm font-semibold tracking-wide transition ${
          tab === "liked" ? "bg-white/10 text-white" : "text-neutral-400 hover:text-neutral-200"
        }`}
        onclick={() => (tab = "liked")}
      >
        Liked
      </button>
    </div>
  {/if}

  <div class={`min-h-0 flex-1 flex-col ${tab === "artists" ? "flex" : "hidden"}`}>
    <ArtistBrowser
      bind:this={artistBrowser}
      {remote}
      bind:inDetail={aInDetail}
      bind:detailName={aName}
      bind:detailHasPartial={aHasPartial}
      bind:detailFull={aFull}
    />
  </div>
  <div class={`min-h-0 flex-1 flex-col ${tab === "playlists" ? "flex" : "hidden"}`}>
    <PlaylistBrowser
      bind:this={plBrowser}
      {remote}
      bind:inDetail={plInDetail}
      bind:detailName={plName}
    />
  </div>
  <div class={`min-h-0 flex-1 flex-col ${tab === "liked" ? "flex" : "hidden"}`}>
    <LikedBrowser {remote} />
  </div>
</div>
