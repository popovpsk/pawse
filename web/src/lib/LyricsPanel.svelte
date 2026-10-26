<script lang="ts">
  import { tick, untrack } from "svelte";
  import type { Remote } from "./connection.svelte";
  import type { LyricsFeed } from "./lyrics/feed.svelte";
  import {
    ACTIVE_TOLERANCE_MS,
    activeRow,
    buildRows,
    fillSpan,
    progress,
    rowEnd,
    sungUntil,
    wordPieces,
    type LyricRow,
    type Piece,
  } from "./lyrics/rows";

  let {
    remote,
    feed,
    canResync = $bindable(false),
  }: { remote: Remote; feed: LyricsFeed; canResync?: boolean } = $props();

  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

  let scroller = $state<HTMLDivElement | null>(null);
  let viewHeight = $state(0);
  let follow = $state(true);
  const rowEls: HTMLElement[] = [];
  let centered: { rows: LyricRow[]; height: number } | null = null;

  const durationMs = $derived(remote.durationMs > 0 ? remote.durationMs : null);
  const rows = $derived(feed.lyrics !== null ? buildRows(feed.lyrics, durationMs) : []);
  const synced = $derived(feed.lyrics?.synced ?? false);
  const activeIx = $derived(synced ? activeRow(rows, remote.positionMs) : null);
  const nowMs = $derived(remote.positionMs + ACTIVE_TOLERANCE_MS);
  const lingerIx = $derived.by(() => {
    if (activeIx === null || activeIx === 0) return null;
    const ix = activeIx - 1;
    const until = sungUntil(rows[ix], rowEnd(rows, ix, durationMs));
    return until !== null && until > nowMs ? ix : null;
  });

  $effect(() => {
    rows;
    untrack(() => (follow = true));
  });

  $effect(() => {
    canResync = synced && activeIx !== null && !follow;
  });

  $effect(() => {
    const el = scroller;
    if (el === null) return;
    el.addEventListener("wheel", release, { passive: true });
    el.addEventListener("touchmove", release, { passive: true });
    return () => {
      el.removeEventListener("wheel", release);
      el.removeEventListener("touchmove", release);
    };
  });

  $effect(() => {
    const ix = activeIx;
    const current = rows;
    const height = viewHeight;
    if (!follow || !synced) return;
    const jump = centered === null || centered.rows !== current || centered.height !== height;
    centered = { rows: current, height };
    tick().then(() => center(ix, !jump));
  });

  function center(ix: number | null, smooth: boolean) {
    if (scroller === null) return;
    const el = ix !== null ? rowEls[ix] : undefined;
    const top = el ? el.offsetTop - scroller.clientHeight / 2 + el.offsetHeight / 2 : 0;
    scroller.scrollTo({ top, behavior: smooth && !reducedMotion.matches ? "smooth" : "instant" });
  }

  export function resync() {
    follow = true;
  }

  function release() {
    if (synced) follow = false;
  }

  function seek(row: LyricRow) {
    if (row.timeMs === null) return;
    follow = true;
    remote.seekTo(row.timeMs);
  }

  function mainPieces(row: LyricRow, ix: number, now: number): Piece[] {
    if (row.words.length > 0) return wordPieces(row.text, row.words, rowEnd(rows, ix, durationMs), now);
    const span = fillSpan(rows, ix, durationMs);
    return [{ text: row.text, fill: span !== null ? progress(now, span[0], span[1]) : 1 }];
  }

  function backingPieces(row: LyricRow, ix: number, now: number): Piece[] {
    const backing = row.background;
    if (backing === null) return [];
    if (backing.words.length === 0) return [{ text: backing.text, fill: 0 }];
    return wordPieces(backing.text, backing.words, rowEnd(rows, ix, durationMs), now);
  }
</script>

{#snippet filled(pieces: Piece[])}
  {#each pieces as piece}<span class="fill" style:--f={piece.fill}>{piece.text}</span>{/each}
{/snippet}

<div class="flex min-h-0 flex-1 flex-col">
  {#if feed.status === "ready" && rows.length > 0}
    <div
      bind:this={scroller}
      bind:clientHeight={viewHeight}
      class="edge-fade relative min-h-0 flex-1 overflow-y-auto px-5 [scrollbar-width:none]"
    >
      {#if synced}
        <div style:padding-top="{viewHeight * 0.4}px" style:padding-bottom="{viewHeight * 0.5}px">
          {#each rows as row, ix (ix)}
            {@const lit = ix === activeIx || ix === lingerIx}
            <button
              bind:this={rowEls[ix]}
              class={`block w-full rounded-xl py-2 text-left text-2xl font-bold leading-snug transition-colors duration-300 can-hover:hover:bg-white/5 lg:text-[22px] ${
                lit ? "text-white" : "text-white/30"
              }`}
              onclick={() => seek(row)}
            >
              {#if row.text === ""}
                <span class="block h-2" aria-hidden="true"></span>
              {:else if lit}
                {@const pieces = mainPieces(row, ix, nowMs)}
                <span>{@render filled(pieces)}</span>
              {:else}
                <span>{row.text}</span>
              {/if}
              {#if row.background !== null}
                <span class="mt-1 block text-[0.72em] font-semibold italic">
                  {#if lit}
                    {@const pieces = backingPieces(row, ix, nowMs)}
                    {@render filled(pieces)}
                  {:else}
                    {row.background.text}
                  {/if}
                </span>
              {/if}
            </button>
          {/each}
        </div>
      {:else}
        <div class="py-6">
          {#each rows as row, ix (ix)}
            <p class="py-1.5 text-lg font-semibold leading-snug text-neutral-200">
              {row.text}
              {#if row.background !== null}
                <span class="block text-[0.8em] italic text-neutral-400">{row.background.text}</span>
              {/if}
            </p>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <div class="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
      {#if feed.status === "error"}
        <p class="text-sm text-neutral-500">Failed to load. Check the connection.</p>
        <button
          class="rounded-full bg-white/10 px-4 py-1.5 text-sm text-neutral-200 transition active:scale-95 hover:bg-white/20"
          onclick={() => feed.retry()}
        >
          Retry
        </button>
      {:else}
        <p class="text-sm text-neutral-500">
          {#if feed.status === "loading"}
            Loading…
          {:else if feed.status === "searching"}
            Searching for lyrics…
          {:else if !remote.hasTrack}
            Nothing playing
          {:else}
            No lyrics
          {/if}
        </p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .fill {
    --lit: rgb(255 255 255);
    --dim: rgb(255 255 255 / 0.45);
    background-image: linear-gradient(
      90deg,
      var(--lit) calc(var(--f) * (100% + 0.6em) - 0.6em),
      var(--dim) calc(var(--f) * (100% + 0.6em))
    );
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    -webkit-box-decoration-break: slice;
    box-decoration-break: slice;
  }

  .edge-fade {
    -webkit-mask-image: linear-gradient(to bottom, transparent, #000 12%, #000 70%, transparent);
    mask-image: linear-gradient(to bottom, transparent, #000 12%, #000 70%, transparent);
  }
</style>
