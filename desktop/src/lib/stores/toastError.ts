// Shared error surface: every catch block used to re-derive this one-liner.
// `errToast(e)` shows the message; pass a prefix for context ("Couldn't save: …").

import { errMsg } from "$lib/invoke";
import { toast } from "./toast.svelte";

export function errToast(e: unknown, prefix?: string) {
  const msg = errMsg(e);
  toast(prefix ? `${prefix}: ${msg}` : `⚠ ${msg}`, "err");
}
