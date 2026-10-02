<script lang="ts">
  let { name, size = 18 }: { name: string; size?: number } = $props();
  const paths: Record<string, string> = {
    accounts: 'M4 5h16v14H4z M4 9h16 M9 9v10',
    automation: 'M13 2 4 14h7l-1 8 10-12h-7z',
    settings:
      'M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1z M9 12a3 3 0 1 0 6 0 3 3 0 1 0-6 0',
    refresh: 'M20 7v5h-5 M4 17v-5h5 M20 12a8 8 0 0 0-14-5 M4 12a8 8 0 0 0 14 5',
    more: 'M5 12h.01 M12 12h.01 M19 12h.01',
    plus: 'M12 5v14 M5 12h14',
    chevron: 'm8 10 4 4 4-4',
    next: 'M5 12h14 m-5-5 5 5-5 5',
    clock: 'M12 3a9 9 0 1 0 0 18 9 9 0 1 0 0-18 M12 7v5l3 2',
    calendar: 'M4 5h16v15H4z M8 3v4 M16 3v4 M4 10h16',
    credits: 'M12 3a9 9 0 1 0 0 18 9 9 0 1 0 0-18 M8 9h7 M8 12h6 M8 15h7',
    import: 'M12 3v12 m-4-4 4 4 4-4 M4 15v6h16v-6',
    archive: 'M4 7h16v14H4z M3 3h18v4H3z M9 11h6',
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.65"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"><path d={paths[name] ?? paths.accounts} /></svg
>
