// Toast store — app-wide feedback replacing the old header banner.
// aria-live region rendered by components/common/Toast.svelte.

export interface Toast {
  id: number;
  message: string;
  kind: "info" | "ok" | "warn" | "err";
}

let nextId = 0;

export const toasts = $state<{ items: Toast[] }>({ items: [] });

/** Push a toast. `kind` only affects coloring/icon; all are announced. */
export function toast(message: string, kind: Toast["kind"] = "info") {
  const id = ++nextId;
  toasts.items.push({ id, message, kind });
  // Auto-dismiss after 3.5s; errors linger a beat longer.
  setTimeout(() => dismissToast(id), kind === "err" ? 5000 : 3500);
  return id;
}

export function dismissToast(id: number) {
  const i = toasts.items.findIndex((t) => t.id === id);
  if (i >= 0) toasts.items.splice(i, 1);
}
