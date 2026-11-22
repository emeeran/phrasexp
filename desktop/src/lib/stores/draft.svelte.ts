// Shared settings-draft helper — the "copy the store values once, edit locally,
// save writes through" pattern (SettingsPage, AiPage). Shallow-copies on first
// sight of the source; `reset` re-arms the copy for reloads.

interface Draft<T> {
  /** The editable copy, or null until the source has a value. */
  readonly draft: T | null;
  /** Drop the copy so the next source change re-copies (after save/reload). */
  reset(): void;
}

export function useDraft<T extends object>(source: () => T | null | undefined): Draft<T> {
  let draft = $state<T | null>(null);

  $effect(() => {
    const v = source();
    if (v && !draft) draft = { ...v };
  });

  return {
    get draft() {
      return draft;
    },
    reset() {
      draft = null;
    },
  };
}
