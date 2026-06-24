<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { Remote, formatTime, type Status } from "./lib/connection.svelte";

  const remote = new Remote();

  let showQueue = $state(false);

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

{#snippet queueList()}
  {#if remote.queue.length === 0}
    <p class="px-3 py-8 text-center text-sm text-neutral-500">Queue is empty</p>
  {:else}
    {#each remote.queue as item, i (i)}
      <button
        class={`flex w-full items-center gap-3 rounded-xl px-3 py-2 text-left transition active:scale-[0.99] ${
          i === remote.queueIndex ? "bg-white/10" : "hover:bg-white/5"
        }`}
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
        {#if i === remote.queueIndex}
          <svg class="h-4 w-4 flex-shrink-0 text-emerald-400" viewBox="0 0 24 24" fill="currentColor">
            <path d="M8 5v14l11-7z" />
          </svg>
        {/if}
      </button>
    {/each}
  {/if}
{/snippet}

<div class="relative flex min-h-[100dvh] flex-col overflow-hidden bg-neutral-950 text-neutral-100 lg:h-[100dvh]">
  {#if remote.coverUrl}
    <img
      src={remote.coverUrl}
      alt=""
      aria-hidden="true"
      class="pointer-events-none absolute inset-0 h-full w-full scale-125 object-cover opacity-40 blur-3xl saturate-150"
    />
    <div class="absolute inset-0 bg-gradient-to-b from-neutral-950/70 via-neutral-950/80 to-neutral-950"></div>
  {/if}

  <header class="relative z-10 flex items-center justify-between px-5 py-4">
    <span class="flex items-center gap-2">
      <img src="/pawse.svg" alt="pawse" class="h-6 w-6 rounded-md" />
      <span class="text-sm font-semibold tracking-wide text-neutral-300">pawse</span>
    </span>
    <span class="flex items-center gap-4">
      <span class="flex items-center gap-2 text-xs text-neutral-400">
        <span class={`h-2 w-2 rounded-full ${dot[remote.status]} transition-colors`}></span>
        <span class="capitalize">{remote.status}</span>
      </span>
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

  <div class="relative z-10 flex min-h-0 flex-1 flex-col lg:flex-row">
  <main class="mx-auto flex w-full max-w-sm flex-1 flex-col items-center justify-center gap-8 px-6 pb-10 lg:max-w-5xl lg:flex-row lg:gap-12 2xl:max-w-6xl 2xl:gap-16">
    <div class="aspect-square w-full overflow-hidden rounded-3xl bg-neutral-800/60 shadow-2xl ring-1 ring-white/10 lg:w-80 lg:flex-shrink-0 xl:w-96 2xl:w-[28rem]">
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

    <div class="flex w-full flex-col gap-8 lg:flex-1 lg:gap-10">
    <div class="w-full text-center lg:text-left">
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

    <div class="flex items-center justify-center gap-10 lg:justify-start">
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
    </div>
    </div>
  </main>

  <aside class="hidden min-h-0 flex-col border-l border-white/10 bg-white/5 backdrop-blur-xl lg:flex lg:w-80 xl:w-96">
    <div class="flex items-center px-5 py-4">
      <h2 class="text-sm font-semibold tracking-wide text-neutral-300">Queue</h2>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
      {@render queueList()}
    </div>
  </aside>
  </div>

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
        {@render queueList()}
      </div>
    </section>
  {/if}
</div>
