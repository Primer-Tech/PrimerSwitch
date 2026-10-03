<script lang="ts">
  let {
    name,
    size = 18,
  }: {
    name: 'terminal' | 'warning' | 'check' | 'swap' | 'trash' | 'info';
    size?: number;
  } = $props();
  const paths = {
    terminal: 'M4 5h16v14H4z M7.5 9.5 10 12l-2.5 2.5 M12.5 15h4',
    warning: 'M12 4 2.8 19.5h18.4z M12 10v4.5 M12 17.2h.01',
    check: 'M5 12.5 9.5 17 19 7.5',
    swap: 'M4 8h15 M15 4l4 4-4 4 M20 16H5 M9 12l-4 4 4 4',
    trash: 'M4.5 7h15 M9.5 7V4.5h5V7 M6.5 7l1 12.5h9l1-12.5',
    info: 'M12 3a9 9 0 1 0 0 18 9 9 0 1 0 0-18 M12 11v5.5 M12 7.8h.01',
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
  aria-hidden="true"><path d={paths[name]} /></svg
>
