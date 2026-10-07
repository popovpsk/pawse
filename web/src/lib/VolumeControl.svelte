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
    class={`flex items-center justify-center transition active:scale-90 hover:text-white ${open ? "text-white" : "text-neutral-400"} ${remote.volumeLocked ? "opacity-50" : ""}`}
    aria-label="Volume"
    onclick={() => (open = !open)}
  >
    <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
      <path d="M3.5 10.2c0-.6.5-1.1 1.1-1.1h2.3l4.2-3.5c.6-.5 1.4-.1 1.4.7v11.4c0 .8-.8 1.2-1.4.7l-4.2-3.5H4.6c-.6 0-1.1-.5-1.1-1.1z" />
      {#if remote.volume <= 0}
        <path d="M16.2 9.7l4.6 4.6M20.8 9.7l-4.6 4.6" />
      {:else if remote.volume < 0.5}
        <path d="M15.8 9.2a4 4 0 0 1 0 5.6" />
      {:else}
        <path d="M15.8 9.2a4 4 0 0 1 0 5.6" />
        <path d="M18.6 6.4a8 8 0 0 1 0 11.2" />
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
