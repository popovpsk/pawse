<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { Remote, formatTime, type Status } from "./lib/connection.svelte";
  import Library from "./lib/Library.svelte";
  import ArtistBrowser from "./lib/ArtistBrowser.svelte";
  import VolumeControl from "./lib/VolumeControl.svelte";

  const remote = new Remote();

  let showQueue = $state(false);
  let showLibrary = $state(false);
  let desktopTab = $state<"queue" | "artists">("queue");

  let paneBrowser = $state<ArtistBrowser | null>(null);
  let paneInDetail = $state(false);
  let paneName = $state("");
  let paneHasPartial = $state(false);
  let paneFull = $state(false);

  const dot: Record<Status, string> = {
    open: "bg-emerald-400",
    connecting: "bg-amber-400",
    reconnecting: "bg-amber-400",
  };

  const progress = $derived(
    remote.durationMs > 0
      ? Math.min(100, (remote.positionMs / remote.durationMs) * 100)
      : 0,
  );

  function onSeekInput(e: Event) {
    const value = Number((e.currentTarget as HTMLInputElement).value);
    remote.previewSeek((value / 1000) * remote.durationMs);
  }

  function onSeekStart() {
    if (remote.durationMs > 0) remote.beginSeek();
  }

  function onSeekEnd(e: Event) {
    const value = Number((e.currentTarget as HTMLInputElement).value);
    remote.endSeek((value / 1000) * remote.durationMs);
  }

</script>

{#snippet queueList(variant: "mobile" | "desktop")}
  {#if remote.queue.length === 0}
    <p class="px-3 py-8 text-center text-sm text-neutral-500">Queue is empty</p>
  {:else}
    {#each remote.queue as item, i (i)}
      <div
        class={`group flex items-center gap-2 rounded-xl pl-3 pr-2 transition ${
          i === remote.queueIndex ? "bg-white/10" : "hover:bg-white/5"
        }`}
      >
        <button
          class="flex min-w-0 flex-1 items-center gap-3 py-2 text-left transition active:scale-[0.99]"
          onclick={() => {
            remote.playAt(i);
            showQueue = false;
          }}
        >
          <div class="h-11 w-11 flex-shrink-0 overflow-hidden rounded-md bg-neutral-800">
            {#if item.cover_id !== null}
              <img src={remote.coverUrlFor(item.cover_id)} alt="" class="h-full w-full object-cover" />
            {:else}
              <div class="flex h-full w-full items-center justify-center text-neutral-600">
                <svg class="h-1/2 w-1/2" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 18V5l12-2v13" />
                  <circle cx="6" cy="18" r="3" />
                  <circle cx="18" cy="16" r="3" />
                </svg>
              </div>
            {/if}
          </div>
          <div class="min-w-0 flex-1">
            <p class={`truncate text-sm ${i === remote.queueIndex ? "font-semibold text-white" : "text-neutral-200"}`}>
              {item.title}
            </p>
            {#if item.artist}
              <p class="truncate text-xs text-neutral-400">{item.artist}</p>
            {/if}
          </div>
        </button>
        {#if i === remote.queueIndex}
          <svg class="h-4 w-4 flex-shrink-0 text-emerald-400" viewBox="0 0 24 24" fill="currentColor">
            <path d="M8 5v14l11-7z" />
          </svg>
        {/if}
        <button
          class={`flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white ${
            variant === "mobile" ? "opacity-100" : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
          }`}
          aria-label="Remove from queue"
          onclick={() => remote.removeAt(i)}
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
          </svg>
        </button>
      </div>
    {/each}
  {/if}
{/snippet}

<div class="relative isolate flex min-h-[100dvh] flex-col overflow-hidden bg-neutral-950 text-neutral-100 lg:h-[100dvh] lg:flex-row">
  {#if remote.coverUrl}
    <img
      src={remote.coverUrl}
      alt=""
      aria-hidden="true"
      class="pointer-events-none absolute inset-0 -z-10 h-full w-full scale-125 object-cover opacity-40 blur-3xl saturate-150"
    />
    <div class="absolute inset-0 -z-10 bg-gradient-to-b from-neutral-950/70 via-neutral-950/80 to-neutral-950"></div>
  {/if}

  <div class="relative flex min-h-0 min-w-0 flex-1 flex-col">
  <header class="relative z-10 flex items-center justify-between gap-2 px-5 py-4">
    <span class="flex min-w-0 items-center gap-2 sm:gap-3">
      <img src="/pawse.svg" alt="pawse" class="h-6 w-6 flex-shrink-0 rounded-md" />
      <span class="hidden text-sm font-semibold tracking-wide text-neutral-300 sm:inline">pawse</span>
      <span class="flex min-w-0 items-center gap-2 text-xs text-neutral-400">
        <span class={`h-2 w-2 flex-shrink-0 rounded-full ${dot[remote.status]} transition-colors`}></span>
        {#if remote.status !== "open"}
          <span class="truncate capitalize">{remote.status}</span>
        {/if}
      </span>
    </span>
    <span class="flex flex-shrink-0 items-center gap-4 sm:gap-5">
      <div class="lg:hidden">
        <VolumeControl {remote} direction="down" />
      </div>
      <button
        class="text-neutral-400 transition active:scale-90 hover:text-white lg:hidden"
        aria-label="Library"
        onclick={() => (showLibrary = true)}
      >
        <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
          <circle cx="12" cy="7" r="4" />
        </svg>
      </button>
      <button
        class="text-neutral-400 transition active:scale-90 hover:text-white lg:hidden"
        aria-label="Queue"
        onclick={() => (showQueue = true)}
      >
        <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <path d="M4 6h12M4 12h12M4 18h8" />
          <path d="M17 14v6l4-3z" fill="currentColor" stroke="none" />
        </svg>
      </button>
    </span>
  </header>

  <main class="flex min-h-0 flex-1 items-center justify-center px-6 py-10">
    <div class="flex w-full min-w-0 max-w-sm flex-col items-center gap-8 lg:max-w-4xl lg:flex-row lg:items-center lg:gap-12 xl:max-w-5xl 2xl:gap-16">
    <div class="aspect-square w-full overflow-hidden rounded-3xl bg-neutral-800/60 shadow-2xl ring-1 ring-white/10 lg:w-80 lg:flex-shrink-0 xl:w-96 2xl:w-[26rem]">
      {#if remote.coverUrl}
        <img src={remote.coverUrl} alt="" class="h-full w-full object-cover" />
      {:else}
        <div class="flex h-full w-full items-center justify-center text-neutral-600">
          <svg class="h-1/3 w-1/3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 18V5l12-2v13" />
            <circle cx="6" cy="18" r="3" />
            <circle cx="18" cy="16" r="3" />
          </svg>
        </div>
      {/if}
    </div>

    <div class="flex w-full min-w-0 flex-col gap-8 lg:flex-1 lg:gap-10">
    <div class="w-full min-w-0 text-center lg:text-left">
      <h1 class="truncate text-2xl font-semibold tracking-tight lg:text-3xl">
        {remote.title ?? "Nothing playing"}
      </h1>
      <p class="mt-1 truncate text-sm text-neutral-400">
        {remote.artist ?? (remote.hasTrack ? "" : "Pick a track on your computer")}
      </p>
      {#if remote.album}
        <p class="mt-0.5 truncate text-sm text-neutral-600">{remote.album}</p>
      {/if}
    </div>

    <div class="w-full">
      <input
        type="range"
        class="seek w-full"
        min="0"
        max="1000"
        step="1"
        value={progress * 10}
        style={`--p:${progress}%`}
        disabled={!remote.hasTrack || remote.durationMs === 0}
        oninput={onSeekInput}
        onpointerdown={onSeekStart}
        onchange={onSeekEnd}
        onpointerup={onSeekEnd}
        onpointercancel={onSeekEnd}
        onlostpointercapture={onSeekEnd}
      />
      <div class="mt-2 flex justify-between text-xs tabular-nums text-neutral-400">
        <span>{formatTime(remote.positionMs)}</span>
        <span>{formatTime(remote.durationMs)}</span>
      </div>
    </div>

    <div class="flex items-center justify-center gap-5 lg:justify-start lg:gap-6">
      <button
        class={`transition active:scale-90 hover:text-white ${remote.shuffle ? "text-emerald-400" : "text-neutral-500"}`}
        aria-label="Shuffle"
        onclick={() => remote.toggleShuffle()}
      >
        <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="16 3 21 3 21 8" />
          <line x1="4" y1="20" x2="21" y2="3" />
          <polyline points="21 16 21 21 16 21" />
          <line x1="15" y1="15" x2="21" y2="21" />
          <line x1="4" y1="4" x2="9" y2="9" />
        </svg>
      </button>
      <button
        class="text-neutral-300 transition active:scale-90 enabled:hover:text-white disabled:opacity-30"
        aria-label="Previous"
        disabled={!remote.hasTrack}
        onclick={() => remote.prev()}
      >
        <svg class="h-8 w-8" viewBox="0 0 24 24" fill="currentColor">
          <path d="M6 5h2v14H6zM20 5v14L9 12z" />
        </svg>
      </button>

      <button
        class="flex h-14 w-14 items-center justify-center rounded-full bg-white text-neutral-900 shadow-lg transition active:scale-90 enabled:hover:scale-105 disabled:opacity-30"
        aria-label={remote.playing ? "Pause" : "Play"}
        disabled={!remote.hasTrack}
        onclick={() => remote.playPause()}
      >
        {#if remote.playing}
          <svg class="h-8 w-8" viewBox="0 0 24 24" fill="currentColor">
            <path d="M7 5h4v14H7zM13 5h4v14h-4z" />
          </svg>
        {:else}
          <svg class="ml-0.5 h-8 w-8" viewBox="0 0 24 24" fill="currentColor">
            <path d="M8 5v14l11-7z" />
          </svg>
        {/if}
      </button>

      <button
        class="text-neutral-300 transition active:scale-90 enabled:hover:text-white disabled:opacity-30"
        aria-label="Next"
        disabled={!remote.hasTrack}
        onclick={() => remote.next()}
      >
        <svg class="h-8 w-8" viewBox="0 0 24 24" fill="currentColor">
          <path d="M16 5h2v14h-2zM4 5l11 7-11 7z" />
        </svg>
      </button>
      <button
        class={`transition active:scale-90 hover:text-white ${remote.repeat !== "off" ? "text-emerald-400" : "text-neutral-500"}`}
        aria-label="Repeat"
        onclick={() => remote.cycleRepeat()}
      >
        <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="17 1 21 5 17 9" />
          <path d="M3 11V9a4 4 0 0 1 4-4h14" />
          <polyline points="7 23 3 19 7 15" />
          <path d="M21 13v2a4 4 0 0 1-4 4H3" />
          {#if remote.repeat === "one"}
            <text x="12" y="15" text-anchor="middle" font-size="8" font-weight="bold" fill="currentColor" stroke="none">1</text>
          {/if}
        </svg>
      </button>
      <div class="hidden lg:ml-auto lg:block">
        <VolumeControl {remote} direction="up" />
      </div>
    </div>
    </div>
    </div>
  </main>
  </div>

  <aside class="hidden min-h-0 flex-col border-l border-white/10 bg-white/5 backdrop-blur-xl lg:flex lg:w-80 xl:w-96">
    <div class="flex items-center gap-1 px-3 py-3">
      <button
        class={`rounded-full px-3 py-1.5 text-sm font-semibold tracking-wide transition ${
          desktopTab === "queue" ? "bg-white/10 text-white" : "text-neutral-400 hover:text-neutral-200"
        }`}
        onclick={() => (desktopTab = "queue")}
      >
        Queue
      </button>
      <button
        class={`rounded-full px-3 py-1.5 text-sm font-semibold tracking-wide transition ${
          desktopTab === "artists" ? "bg-white/10 text-white" : "text-neutral-400 hover:text-neutral-200"
        }`}
        onclick={() => (desktopTab = "artists")}
      >
        Artists
      </button>
    </div>
    <div class={`min-h-0 flex-1 overflow-y-auto px-2 pb-4 ${desktopTab === "queue" ? "" : "hidden"}`}>
      {@render queueList("desktop")}
    </div>
    <div class={`min-h-0 flex-1 flex-col ${desktopTab === "artists" ? "flex" : "hidden"}`}>
      {#if paneInDetail}
        <div class="flex items-center gap-2 border-y border-white/10 px-2 py-2">
          <button
            class="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full text-neutral-300 transition active:scale-90 hover:bg-white/10"
            aria-label="Back to artists"
            onclick={() => paneBrowser?.goBack()}
          >
            <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M15 18l-6-6 6-6" />
            </svg>
          </button>
          <span class="min-w-0 flex-1 truncate text-sm font-semibold">{paneName}</span>
          {#if paneHasPartial}
            <button
              class={`flex h-8 flex-shrink-0 items-center gap-1.5 rounded-full px-3 text-xs font-medium transition active:scale-95 ${
                paneFull ? "bg-emerald-400 text-neutral-950" : "bg-white/10 text-neutral-300"
              }`}
              onclick={() => paneBrowser?.toggleFull()}
            >
              <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="12" cy="12" r="9" />
                <circle cx="12" cy="12" r="3" />
              </svg>
              Full
            </button>
          {/if}
        </div>
      {/if}
      <ArtistBrowser
        bind:this={paneBrowser}
        {remote}
        bind:inDetail={paneInDetail}
        bind:detailName={paneName}
        bind:detailHasPartial={paneHasPartial}
        bind:detailFull={paneFull}
      />
    </div>
  </aside>

  {#if showQueue}
    <button
      class="absolute inset-0 z-20 cursor-default bg-black/50 lg:hidden"
      aria-label="Close queue"
      transition:fade={{ duration: 150 }}
      onclick={() => (showQueue = false)}
    ></button>
    <section
      class="absolute inset-x-0 bottom-0 z-30 flex max-h-[80dvh] flex-col rounded-t-3xl border-t border-white/10 bg-neutral-900 shadow-2xl lg:hidden"
      transition:fly={{ y: 500, duration: 250 }}
    >
      <div class="flex items-center justify-between px-5 py-4">
        <h2 class="text-sm font-semibold tracking-wide text-neutral-300">Queue</h2>
        <button
          class="text-neutral-400 transition active:scale-90 hover:text-white"
          aria-label="Close"
          onclick={() => (showQueue = false)}
        >
          <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-[max(1rem,env(safe-area-inset-bottom))]">
        {@render queueList("mobile")}
      </div>
    </section>
  {/if}

  {#if showLibrary}
    <Library {remote} onclose={() => (showLibrary = false)} />
  {/if}
</div>
