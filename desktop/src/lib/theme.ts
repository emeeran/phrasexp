/** Theme management — persisted in localStorage, applied via data-theme on <html>. */

export type Theme = "dark" | "light";

export function getTheme(): Theme {
  return (localStorage.getItem("phrasexp-theme") as Theme) || "dark";
}

export function applyTheme(): Theme {
  const theme = getTheme();
  document.documentElement.dataset.theme = theme;
  return theme;
}

export function toggleTheme(): Theme {
  const next: Theme = getTheme() === "dark" ? "light" : "dark";
  localStorage.setItem("phrasexp-theme", next);
  document.documentElement.dataset.theme = next;
  return next;
}
