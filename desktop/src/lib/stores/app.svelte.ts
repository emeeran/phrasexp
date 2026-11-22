// App store — navigation route + cross-cutting UI state.
// SPA view-switching: five flat destinations, persisted to localStorage.

import type { Phrase, PhraseSummary, PhraseVersion } from "$lib/types";

export type Route = "library" | "clipboard" | "macros" | "ai" | "settings";

export interface EditingPhrase {
  phrase: Phrase | null;
  folderId: string;
  initialText?: string;
}

export type ModalKind = "dupes" | "history" | null;

function initialRoute(): Route {
  const saved = localStorage.getItem("phrasexp-route");
  const valid: Route[] = ["library", "clipboard", "macros", "ai", "settings"];
  return valid.includes(saved as Route) ? (saved as Route) : "library";
}

export const app = $state({
  route: initialRoute(),
  /** Editing session in the Library detail pane; null = show Test panel. */
  editing: null as EditingPhrase | null,
  /** Which modal overlay is open (Duplicates / History). */
  modal: null as ModalKind,
  /** Phrase whose version history the modal shows (when modal === "history"). */
  historyPhrase: null as PhraseSummary | null,
  /** Version snapshots for the history modal. */
  versions: [] as PhraseVersion[],
  /** Duplicate groups for the dupes modal. */
  dupGroups: [] as PhraseSummary[][],

  routeTo(r: Route) {
    this.route = r;
    localStorage.setItem("phrasexp-route", r);
  },
});
