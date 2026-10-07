<script lang="ts" generics="T extends string">
  type Option = { value: T; label: string; hints: [string, string]; defaultDesc?: boolean };

  let {
    options,
    sort,
    desc,
    onchange,
  }: {
    options: Option[];
    sort: T;
    desc: boolean;
    onchange: (sort: T, desc: boolean) => void;
  } = $props();

  const hint = $derived(options.find((o) => o.value === sort)?.hints[desc ? 1 : 0] ?? "");

  function choose(option: Option) {
    if (option.value === sort) onchange(sort, !desc);
    else onchange(option.value, option.defaultDesc ?? false);
  }
</script>

<div class="flex items-center gap-2 px-3 py-2 lg:pb-2 lg:pt-1">
  <div class="flex min-w-0 rounded-full bg-white/5 p-0.5" role="group" aria-label="Sort by">
    {#each options as option (option.value)}
      <button
        class={`flex-shrink-0 rounded-full px-3.5 py-2 text-sm font-medium transition active:scale-95 lg:px-2.5 lg:py-1 lg:text-xs ${
          option.value === sort ? "bg-white/15 text-white" : "text-neutral-400 hover:text-neutral-200"
        }`}
        aria-pressed={option.value === sort}
        onclick={() => choose(option)}
      >
        {option.label}
      </button>
    {/each}
  </div>
  <button
    class="ml-auto flex min-w-0 items-center gap-1.5 rounded-full px-2 py-2 text-sm text-neutral-400 transition active:scale-95 hover:text-white lg:gap-1 lg:py-1 lg:text-xs"
    aria-label={`Order: ${hint}. Reverse`}
    onclick={() => onchange(sort, !desc)}
  >
    <svg class="h-4 w-4 flex-shrink-0 lg:h-3.5 lg:w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M7 4v16M3 8l4-4 4 4M17 20V4M13 16l4 4 4-4" />
    </svg>
    <span class="truncate">{hint}</span>
  </button>
</div>
