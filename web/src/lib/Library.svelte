<script lang="ts">
  import { fade } from "svelte/transition";
  import { Remote } from "./connection.svelte";
  import ArtistBrowser from "./ArtistBrowser.svelte";

  let { remote, onclose }: { remote: Remote; onclose: () => void } = $props();

  let browser = $state<ArtistBrowser | null>(null);
  let inDetail = $state(false);
  let detailName = $state("");
  let detailHasPartial = $state(false);
  let detailFull = $state(false);
</script>

<div
  class="absolute inset-0 z-40 flex flex-col bg-neutral-950 text-neutral-100"
  transition:fade={{ duration: 150 }}
>
  <header class="flex items-center gap-3 border-b border-white/10 px-4 py-3">
    {#if inDetail}
      <button
        class="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-full text-neutral-300 transition active:scale-90 hover:bg-white/10"
        aria-label="Back to artists"
        onclick={() => browser?.goBack()}
      >
        <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M15 18l-6-6 6-6" />
        </svg>
      </button>
    {/if}
    <h2 class="min-w-0 flex-1 truncate text-base font-semibold">
      {inDetail ? detailName : "Artists"}
    </h2>
    {#if inDetail && detailHasPartial}
      <button
        class={`flex h-10 flex-shrink-0 items-center gap-2 rounded-full px-4 text-xs font-medium transition active:scale-95 ${
          detailFull ? "bg-emerald-400 text-neutral-950" : "bg-white/10 text-neutral-300"
        }`}
        onclick={() => browser?.toggleFull()}
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

  <ArtistBrowser
    bind:this={browser}
    {remote}
    bind:inDetail
    bind:detailName
    bind:detailHasPartial
    bind:detailFull
  />
</div>
