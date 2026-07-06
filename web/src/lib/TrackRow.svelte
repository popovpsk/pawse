<script lang="ts">
  import { Remote, formatTime, type PlaylistTrack } from "./connection.svelte";

  let {
    remote,
    track,
    onplay,
    onqueue,
  }: {
    remote: Remote;
    track: PlaylistTrack;
    onplay: () => void;
    onqueue: () => void;
  } = $props();

  const active = $derived(track.id === remote.currentTrackId);
</script>

<div
  class={`group flex w-full items-center rounded-xl transition hover:bg-white/5 ${
    active ? "bg-white/10" : ""
  }`}
>
  <button
    class="flex min-w-0 flex-1 items-center gap-3 py-2 pl-3 pr-2 text-left"
    onclick={onplay}
  >
    <div class="h-11 w-11 flex-shrink-0 overflow-hidden rounded-md bg-neutral-800">
      {#if track.cover_id !== null}
        <img src={remote.coverUrlFor(track.cover_id)} alt="" loading="lazy" class="h-full w-full object-cover" />
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
      <p class={`truncate text-sm ${active ? "font-semibold text-white" : "text-neutral-200"}`}>
        {track.title}
      </p>
      {#if track.artist}
        <p class="truncate text-xs text-neutral-400">{track.artist}</p>
      {/if}
    </div>
    <span class="flex-shrink-0 text-xs tabular-nums text-neutral-500">
      {formatTime(track.duration_ms)}
    </span>
  </button>
  <button
    class="mr-1 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:focus-visible:opacity-100 [@media(hover:hover)]:group-hover:opacity-100"
    aria-label="Add to queue"
    onclick={onqueue}
  >
    <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
      <path d="M4 6h9M4 12h9M4 18h6" />
      <path d="M18 9v6M15 12h6" />
    </svg>
  </button>
</div>
