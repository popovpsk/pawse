<script lang="ts">
  import { fade } from "svelte/transition";
  import { Remote } from "./connection.svelte";

  let { remote, direction }: { remote: Remote; direction: "down" | "up" } = $props();

  let open = $state(false);

  function onInput(e: Event) {
    const value = Number((e.currentTarget as HTMLInputElement).value);
    remote.previewVolume(value / 100);
  }

  function onEnd(e: Event) {
    const value = Number((e.currentTarget as HTMLInputElement).value);
    remote.endVolume(value / 100);
  }
</script>

{#if open}
  <button
    class="fixed inset-0 z-40 cursor-default"
    aria-label="Close volume"
    onclick={() => (open = false)}
  ></button>
{/if}

<div class="relative">
  <button
    class={`flex items-center justify-center transition active:scale-90 hover:text-white ${open ? "text-white" : "text-neutral-400"}`}
    aria-label="Volume"
    onclick={() => (open = !open)}
  >
    <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M11 5L6 9H2v6h4l5 4V5z" fill="currentColor" stroke="none" />
      {#if remote.volume <= 0}
        <line x1="17" y1="9" x2="23" y2="15" />
        <line x1="23" y1="9" x2="17" y2="15" />
      {:else if remote.volume < 0.5}
        <path d="M15.5 8.5a5 5 0 0 1 0 7" />
      {:else}
        <path d="M15.5 8.5a5 5 0 0 1 0 7" />
        <path d="M18.5 5.5a9 9 0 0 1 0 13" />
      {/if}
    </svg>
  </button>
  {#if open}
    <div
      class={`absolute left-1/2 z-50 -translate-x-1/2 rounded-full border border-white/10 bg-neutral-900 px-1.5 py-4 shadow-2xl ${
        direction === "down" ? "top-full mt-2" : "bottom-full mb-2"
      }`}
      transition:fade={{ duration: 120 }}
    >
      <div class="flex h-28 w-7 items-center justify-center">
        <input
          type="range"
          class="seek w-28 flex-shrink-0 -rotate-90"
          min="0"
          max="100"
          step="1"
          value={remote.volume * 100}
          style={`--p:${remote.volume * 100}%`}
          disabled={remote.volumeLocked}
          oninput={onInput}
          onchange={onEnd}
          onpointerup={onEnd}
          onpointercancel={onEnd}
        />
      </div>
    </div>
  {/if}
</div>
