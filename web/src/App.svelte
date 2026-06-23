<script lang="ts">
  import { Remote, type Status } from "./lib/connection.svelte";

  const remote = new Remote();

  const dot: Record<Status, string> = {
    open: "bg-emerald-500",
    connecting: "bg-amber-500",
    reconnecting: "bg-amber-500",
  };
</script>

<main
  class="flex min-h-screen flex-col items-center justify-center gap-6 bg-neutral-950 text-neutral-100"
>
  <div class="flex items-center gap-2 text-sm text-neutral-400">
    <span class={`h-2 w-2 rounded-full ${dot[remote.status]}`}></span>
    {remote.status}
  </div>
  <div class="flex items-center gap-3">
    {#if remote.title}
      <span class="text-xl text-neutral-500">{remote.playing ? "▶" : "⏸"}</span>
    {/if}
    <span class="text-3xl font-semibold tracking-tight">{remote.title ?? "—"}</span>
  </div>
</main>
