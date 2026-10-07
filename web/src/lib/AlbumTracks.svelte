<script lang="ts">
  import { Remote, formatTime, type AlbumTrack } from "./connection.svelte";

  let {
    remote,
    tracks,
    onplay,
    onqueue,
  }: {
    remote: Remote;
    tracks: AlbumTrack[];
    onplay: (track: AlbumTrack) => void;
    onqueue: (track: AlbumTrack) => void;
  } = $props();

  type Row = { kind: "disc"; disc: number } | { kind: "track"; track: AlbumTrack };

  const rows = $derived.by((): Row[] => {
    const multiDisc = tracks.some((t) => t.disc_number > 1);
    if (!multiDisc) return tracks.map((track) => ({ kind: "track", track }));
    const out: Row[] = [];
    let disc = 0;
    for (const track of tracks) {
      if (track.disc_number !== disc) {
        disc = track.disc_number;
        out.push({ kind: "disc", disc });
      }
      out.push({ kind: "track", track });
    }
    return out;
  });
</script>

{#each rows as row, rowIx (rowIx)}
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
        onclick={() => onplay(row.track)}
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
        class="mr-1 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-full text-neutral-400 transition active:scale-90 hover:bg-white/10 hover:text-white can-hover:opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
        aria-label="Add to queue"
        onclick={() => onqueue(row.track)}
      >
        <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <path d="M4 6h9M4 12h9M4 18h6" />
          <path d="M18 9v6M15 12h6" />
        </svg>
      </button>
    </div>
  {/if}
{/each}
