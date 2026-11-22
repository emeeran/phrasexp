<script lang="ts" module>
  // Inline SVG icon set — one component, name-keyed. Stroke-based, 16px grid,
  // inherits currentColor. Keeps the bundle free of an icon-font dependency.
  export type IconName =
    | "library"
    | "clipboard"
    | "macros"
    | "ai"
    | "settings"
    | "theme"
    | "sun"
    | "book"
    | "close"
    | "plus"
    | "copy"
    | "trash"
    | "search"
    | "check"
    | "chevron"
    | "bolt"
    | "eye"
    | "info";

  const paths: Record<IconName, string> = {
    library:
      "M4 3h5a2 2 0 0 1 2 2v12a2 2 0 0 0-2-2H4zM12 3h5v12h-5a2 2 0 0 0-2 2V5a2 2 0 0 1 2-2z",
    clipboard:
      "M9 3h4a1 1 0 0 1 1 1v1h3v3H5V5h3V4a1 1 0 0 1 1-1zM5 8h13v10a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1z",
    macros: "M13 4l4 4-8.5 8.5a2 2 0 0 1-1 .5L4 18l1-3.5a2 2 0 0 1 .5-1zM11 6l4 4",
    ai:
      "M12 3l1.7 4.3L18 9l-4.3 1.7L12 15l-1.7-4.3L6 9l4.3-1.7zM18.5 14.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z",
    settings:
      "M12 15.5A3.5 3.5 0 1 0 12 8.5a3.5 3.5 0 0 0 0 7zM19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.9.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
    theme: "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z",
    sun:
      "M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10zM12 1v2M12 21v2M4.2 4.2l1.4 1.4M18.4 18.4l1.4 1.4M1 12h2M21 12h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4",
    book:
      "M4 19.5A2.5 2.5 0 0 1 6.5 17H20M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z",
    close: "M18 6L6 18M6 6l12 12",
    plus: "M12 5v14M5 12h14",
    copy:
      "M9 9h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1zM5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1",
    trash:
      "M4 7h16M9 7V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v3m-9 0l1 13a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-13",
    search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3",
    check: "M20 6L9 17l-5-5",
    chevron: "M9 18l6-6-6-6",
    bolt: "M13 2L3 14h7l-1 8 10-12h-7z",
    eye: "M1 12s4-7 11-7 11 7 11 7-4 7-11 7S1 12 1 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
    info: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 16v-4M12 8h.01",
  };
</script>

<script lang="ts">
  let {
    name,
    size = 16,
    stroke = 1.75,
  }: { name: IconName; size?: number; stroke?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name]} />
</svg>
