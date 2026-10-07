<script lang="ts">
  import { fade } from "svelte/transition";
  import { Remote } from "./connection.svelte";
  import AlbumBrowser from "./AlbumBrowser.svelte";
  import ArtistBrowser from "./ArtistBrowser.svelte";
  import PlaylistBrowser from "./PlaylistBrowser.svelte";
  import LikedBrowser from "./LikedBrowser.svelte";

  let { remote, onclose }: { remote: Remote; onclose: () => void } = $props();

  let tab = $state<"albums" | "artists" | "playlists" | "liked">("albums");

  let albumBrowser = $state<AlbumBrowser | null>(null);
  let alInDetail = $state(false);
  let alName = $state("");

  let artistBrowser = $state<ArtistBrowser | null>(null);
  let aInDetail = $state(false);
  let aName = $state("");
  let aHasPartial = $state(false);
  let aFull = $state(false);

  let plBrowser = $state<PlaylistBrowser | null>(null);
  let plInDetail = $state(false);
  let plName = $state("");

  const activeInDetail = $derived(
    tab === "albums"
      ? alInDetail
      : tab === "artists"
        ? aInDetail
        : tab === "playlists"
          ? plInDetail
          : false,
  );

  const tabs = [
    { id: "albums", label: "Albums" },
    { id: "artists", label: "Artists" },
    { id: "playlists", label: "Playlists" },
    { id: "liked", label: "Liked" },
  ] as const;

  const detailTitle = $derived(
    tab === "albums" ? alName : tab === "artists" ? aName : tab === "playlists" ? plName : "",
  );

  function goBack() {
    if (tab === "albums") albumBrowser?.goBack();
    else if (tab === "artists") artistBrowser?.goBack();
    else if (tab === "playlists") plBrowser?.goBack();
  }
</script>

{#snippet closeButton()}
  <button
    class="flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white"
    aria-label="Close"
    onclick={onclose}
  >
    <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
      <path d="M6 6l12 12M18 6L6 18" />
    </svg>
  </button>
{/snippet}

<div
  class="absolute inset-0 z-40 flex flex-col bg-neutral-950 text-neutral-100"
  transition:fade={{ duration: 150 }}
>
  {#if activeInDetail}
    <header class="flex items-center gap-3 border-b border-white/10 px-4 py-3">
      <button
        class="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-full text-neutral-300 transition active:scale-90 hover:bg-white/10"
        aria-label="Back"
        onclick={goBack}
      >
        <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M15 18l-6-6 6-6" />
        </svg>
      </button>
      <h2 class="min-w-0 flex-1 truncate text-base font-semibold">{detailTitle}</h2>
      {#if tab === "artists" && aHasPartial}
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
      {@render closeButton()}
    </header>
  {:else}
    <div class="flex items-center gap-1 border-b border-white/10 py-2 pl-2 pr-1.5">
      {#each tabs as t (t.id)}
        <button
          class={`h-11 min-w-0 flex-1 truncate rounded-full px-1 text-[15px] font-semibold transition active:scale-95 max-[359px]:px-0 max-[359px]:text-sm ${
            tab === t.id ? "bg-white/10 text-white" : "text-neutral-400 hover:text-neutral-200"
          }`}
          onclick={() => (tab = t.id)}
        >
          {t.label}
        </button>
      {/each}
      {@render closeButton()}
    </div>
  {/if}

  <div class={`min-h-0 flex-1 flex-col ${tab === "albums" ? "flex" : "hidden"}`}>
    <AlbumBrowser
      bind:this={albumBrowser}
      {remote}
      bind:inDetail={alInDetail}
      bind:detailName={alName}
    />
  </div>
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
