<script lang="ts">
  // Popup orchestrator — owns the mode state machine, all Tauri listeners, and the
  // hide→120ms-settle→inject sequence. Presentation lives in the mode components.
  // The window itself is draggable (grip bar + header drag regions) and its height
  // adapts to the mode's content via a ResizeObserver → resize_popup.
  // ⚠ Nav keys in offer mode arrive via the rdev-routed `offer-key` event — do not
  // move this to DOM keydown (X focus/grab cannot deliver them).
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { fly } from "svelte/transition";
  import { api, errMsg } from "$lib/invoke";
  import { fieldsOf, defaultsFor } from "$lib/macros";
  import { paramTokens, lastParams, rememberParams } from "$lib/aiArgs";
  import { rankPhrases } from "$lib/fuzzy";
  import { charDiff } from "./diff";
  import PopupSearch from "./PopupSearch.svelte";
  import PopupInputs from "./PopupInputs.svelte";
  import PopupChoose from "./PopupChoose.svelte";
  import PopupOffer from "./PopupOffer.svelte";
  import PopupAi from "./PopupAi.svelte";
  import PopupAiArg from "./PopupAiArg.svelte";
  import PopupAiResult from "./PopupAiResult.svelte";
  import type { AiAction, Phrase, PhraseSummary } from "$lib/types";

  const win = getCurrentWindow();

  // Fixed size for the airesult view (see the sizing effect below).
  const RESULT_W = 560;
  const RESULT_H = 420;

  let phrases = $state<PhraseSummary[]>([]);
  let q = $state("");
  let selected = $state(0);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let search: PopupSearch | undefined = $state();
  let aiResultView: PopupAiResult | undefined = $state();
  let shell: HTMLElement | undefined = $state();

  // search | inputs (filling input-form fields) | choose (multi-match picker) | offer (spellcheck) | ai (selection rewrite) | aiarg ({language} prompt) | airesult (decide on a rewrite)
  let mode = $state<"search" | "inputs" | "choose" | "offer" | "ai" | "aiarg" | "airesult">(
    "search",
  );
  let current = $state<Phrase | null>(null);
  let formInputs = $state<Record<string, string>>({});
  let candidates = $state<PhraseSummary[]>([]);
  let chooseSelected = $state(0);
  let offerOriginal = $state("");
  let offerCandidates = $state<string[]>([]);
  let offerSelected = $state(0);

  // Selection-AI mode: actions + highlighted row + a truncated preview of the captured
  // selection. The selection itself lives in the Rust-side slot; run_ai_action reads it
  // by action only — it never round-trips through the webview.
  let aiActions = $state<AiAction[]>([]);
  let aiSelected = $state(0);
  let aiPreview = $state("");
  // The action awaiting its parameter answers in aiarg mode (its template's
  // interactive `{token}`s — `{language}`, or `{length}` + `{format}`, …).
  let aiArgAction = $state<AiAction | null>(null);
  let aiArgTokens = $state<string[]>([]);
  let aiParams = $state<Record<string, string>>({});
  // Result mode: the rewrite Rust is holding in its pending slot, awaiting
  // replace / insert / retry / discard (ai_decide). While `streaming`, the text
  // is still arriving via ai-delta events and only Cancel is offered. `aiOriginal`
  // is the captured selection — feeds the diff preview in the result view.
  let aiResult = $state("");
  let aiOriginal = $state("");
  let streaming = $state(false);

  // Adaptive sizing: report the shell's rendered size so the window hugs the
  // mode's content (scrollable regions cap it). Width mirrors the viewport and
  // round-trips unchanged; only height actually moves the window.
  // Exception — airesult keeps ONE stable size (set once on entry): the response
  // area scrolls inside the window instead of it chasing every streaming delta
  // and option pick. The .fill class stretches the shell to that window.
  let lastResize = "";
  $effect(() => {
    if (mode === "airesult") {
      void api.resizePopup(RESULT_W, RESULT_H);
      return;
    }
    const el = shell;
    if (!el) return;
    const report = () => {
      const r = el.getBoundingClientRect();
      const key = `${Math.ceil(r.width)}x${Math.ceil(r.height)}`;
      if (key === lastResize) return; // no echo round-trips
      lastResize = key;
      void api.resizePopup(Math.ceil(r.width), Math.ceil(r.height));
    };
    const ro = new ResizeObserver(report);
    ro.observe(el);
    report();
    return () => ro.disconnect();
  });

  // Fuzzy subsequence ranking (abbreviation hits float to the top, then score,
  // then usage count once stats exist). Empty query → full list, server order.
  const filtered = $derived(q.trim() ? rankPhrases(q, phrases, 50) : phrases);

  // Live diff preview for the currently-selected spellcheck candidate. Recomputes on
  // navigation (offerSelected) so the preview tracks the highlighted row.
  const offerPreview = $derived(
    offerCandidates[offerSelected]
      ? charDiff(offerOriginal, offerCandidates[offerSelected])
      : { left: [], right: [] },
  );

  async function load() {
    try {
      phrases = await api.listPhrases();
      selected = 0;
    } catch (e) {
      error = errMsg(e);
    }
  }

  async function injectById(id: string, inputs: Record<string, string> = {}) {
    await settleThen(() => api.injectPhrase(id, inputs));
  }

  async function pick(p: PhraseSummary) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const full = await api.getPhrase(p.id);
      const fields = fieldsOf(full);
      if (fields.length === 0) {
        await injectById(p.id);
      } else {
        current = full;
        formInputs = defaultsFor(fields);
        mode = "inputs";
        await tick();
      }
    } catch (e) {
      error = errMsg(e);
    } finally {
      busy = false;
    }
  }

  async function confirmInputs() {
    if (!current || busy) return;
    busy = true;
    error = null;
    try {
      await injectById(current.id, formInputs);
    } catch (e) {
      error = errMsg(e);
    } finally {
      busy = false;
    }
  }

  async function onChoose(ids: string[]) {
    const all = await api.listPhrases();
    candidates = all.filter((p) => ids.includes(p.id));
    chooseSelected = 0;
    mode = "choose";
    await tick();
    // Grab keyboard focus now that the window is mapped + rendered (re-grabbing here is the
    // reliable moment — the early grab in show_spellcheck_offer can race window mapping).
    void api.focusPopup();
  }

  // Offer-mode (spellcheck delivery = "offer"): replace, don't stack, on repeated offers.
  // The backend has already sized, positioned, shown, and focused the window.
  async function onOffer(payload: { original: string; candidates: string[] }) {
    offerOriginal = payload.original;
    offerCandidates = payload.candidates;
    offerSelected = 0;
    mode = "offer";
    await tick();
    // Focus first (makes the window viewable/active), then take an active keyboard grab so all
    // keys route to the popup until Enter/Esc dismisses it — no mouse needed.
    await api.focusPopup();
    await api.grabKeyboard();
  }

  // Shared offer plumbing: busy-guard + error surface + offer-state reset. The
  // per-offer sequences below differ on purpose (ignoreOffer hides last, no settle).
  async function runOffer(action: () => Promise<void>) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await action();
    } catch (e) {
      error = errMsg(e);
    } finally {
      busy = false;
      offerOriginal = "";
      offerCandidates = [];
      mode = "search";
    }
  }

  // Release the grab + hide so focus returns to the target app, settle, then act —
  // the backspaces/typing must land in the target field, not the popup.
  async function settleThen(action: () => Promise<unknown>) {
    await api.releaseKeyboard();
    await win.hide();
    await new Promise((r) => setTimeout(r, 120));
    await action();
  }

  // Accept: inject the correction.
  async function acceptOffer(cand: string) {
    await runOffer(() => settleThen(() => api.applyCorrection(cand)));
  }

  // "Add to dictionary": persist so the word won't be offered again.
  async function ignoreOffer() {
    await runOffer(async () => {
      await api.releaseKeyboard();
      await api.spellcheckIgnoreWord(offerOriginal);
      await win.hide();
    });
  }

  // "Delete typed word" (Backspace while the popup is open): erase the whole word
  // + its trailing space.
  async function deleteOfferWord() {
    await runOffer(() => settleThen(() => api.deleteOfferWord()));
  }

  // Selection-AI: the backend captured the selection (copied via the clipboard) and
  // showed this popup. Fetch the action list, switch to ai mode. A capture failure
  // arrives as the same event with `failed` set — the payload is the message.
  async function onAiSelection(preview: string, failed = false) {
    aiPreview = failed ? `⚠ ${preview}` : preview;
    try {
      aiActions = await api.listAiActions();
    } catch (e) {
      error = errMsg(e);
      return;
    }
    if (aiActions.length === 0) {
      error = "No AI actions defined — add one in Settings → AI";
      return;
    }
    aiSelected = 0;
    mode = "ai";
    await tick();
    void api.focusPopup();
  }

  // The template's interactive placeholder + prompt copy live in $lib/aiArgs
  // (pure, unit-tested, mirrors the Rust-side param_token rule).

  // Run the picked action. Actions whose template carries interactive placeholders
  // go through the aiarg prompt first (pre-filled with the last-used answers —
  // running "Paraphrase → academic" again is one Enter). Rust streams the rewrite
  // via ai-delta events and finishes with the pending slot + ai-result event —
  // nothing is pasted until the user decides via decideAi. The popup stays up.
  function runAi(a: AiAction) {
    if (busy || !a) return;
    const tokens = paramTokens(a.prompt_template);
    if (tokens.length > 0) {
      const last = lastParams(a.id);
      aiArgAction = a;
      aiArgTokens = tokens;
      aiParams = Object.fromEntries(tokens.map((t) => [t, last[t] ?? ""]));
      mode = "aiarg";
      return;
    }
    void doRunAi(a.id, undefined);
  }

  async function doRunAi(actionId: string, params?: Record<string, string>) {
    busy = true;
    error = null;
    try {
      await api.runAiAction(actionId, params);
    } catch (e) {
      // Rust re-showed the popup with the failure message; fall back to the list.
      error = errMsg(e);
      mode = "search";
    } finally {
      busy = false;
    }
  }

  // Resolve the result popup. While streaming, the only decision is discard =
  // cancel the in-flight request (Rust restores the clipboard and emits
  // ai-cancelled, which hides the popup). Retry keeps the popup up and swaps in
  // the fresh output; replace/insert release keys + hide first so the paste lands
  // in the target app (replace overwrites the still-active selection, insert
  // pastes after it); copy just drops the text on the clipboard; discard restores
  // the user's prior clipboard and closes. `text` carries the picked option of a
  // multi-option response — Rust acts on it instead of the full result.
  async function decideAi(d: "replace" | "insert" | "copy" | "retry" | "discard", text?: string) {
    if (busy) return;
    if (streaming) {
      if (d === "discard") void api.aiCancel();
      return;
    }
    busy = true;
    error = null;
    try {
      if (d === "retry") {
        streaming = true; // retry streams too — deltas swap the text in place
        aiResult = await api.aiDecide("retry");
      } else if (d === "copy") {
        await api.aiDecide("copy", text);
        mode = "search";
        await win.hide();
      } else {
        await settleThen(() => api.aiDecide(d, text));
        mode = "search";
      }
    } catch (e) {
      // The window is already hidden on the terminal paths; keep result mode so the
      // message + pending result are visible if the popup is re-shown.
      error = errMsg(e);
    } finally {
      busy = false;
      streaming = false;
    }
  }

  async function reset() {
    q = "";
    selected = 0;
    await tick();
    search?.focus();
  }

  // Handle a nav key routed from the rdev listener via the `offer-key` event. rdev observes
  // keys globally, so this works even when X focus/grab don't deliver to the webview DOM —
  // which is why the popup otherwise "doesn't focus" / Escape won't exit. Sole handler for
  // offer mode (onkeydown skips offer mode to avoid double-handling non-idempotent moves).
  function handleOfferKey(name: string) {
    if (mode !== "offer") return;
    if (name === "up") {
      offerSelected = Math.max(offerSelected - 1, 0);
    } else if (name === "down") {
      offerSelected = Math.min(offerSelected + 1, Math.max(offerCandidates.length - 1, 0));
    } else if (name === "enter") {
      const cand = offerCandidates[offerSelected];
      if (cand) void acceptOffer(cand);
    } else if (name === "escape") {
      void api.releaseKeyboard();
      void win.hide();
    } else if (name === "backspace") {
      void deleteOfferWord();
    } else if (/^[1-9]$/.test(name)) {
      const cand = offerCandidates[Number(name) - 1];
      if (cand) void acceptOffer(cand);
    } else if (name === "d" || name === "plus") {
      void ignoreOffer();
    }
  }

  function onkeydown(e: KeyboardEvent) {
    // Offer keys arrive via the rdev-routed `offer-key` event (handleOfferKey); skip DOM
    // handling here so moves aren't applied twice.
    if (mode === "offer") return;
    // Result mode first: Esc here means Discard (mid-stream: Cancel), not the
    // generic hide below.
    if (mode === "airesult") {
      // With multi-option responses unpicked, replace/insert/copy are gated the same
      // way their buttons are (canDecide) — Enter must not paste the raw option list.
      // canDecide also stands down while tokens are still streaming in.
      const ok = aiResultView?.canDecide() ?? !streaming;
      if (e.key === "Enter" && ok) {
        e.preventDefault();
        void decideAi("replace");
      } else if (e.key === "Escape") {
        e.preventDefault();
        void decideAi("discard");
      } else if ((e.key === "i" || e.key === "I") && ok) {
        e.preventDefault();
        void decideAi("insert");
      } else if ((e.key === "c" || e.key === "C") && ok) {
        e.preventDefault();
        // Same path as the button: a highlighted portion of the response wins.
        if (aiResultView) aiResultView.decideCopy();
        else void decideAi("copy");
      } else if ((e.key === "r" || e.key === "R") && ok) {
        e.preventDefault();
        void decideAi("retry");
      }
      return;
    }
    // AI arg mode: the inputs handle Enter/ArrowUp/Down themselves (per-token);
    // here only Esc returns to the actions.
    if (mode === "aiarg") {
      if (e.key === "Escape") {
        e.preventDefault();
        mode = "ai";
        aiArgAction = null;
      }
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (mode === "inputs" || mode === "choose") {
        mode = "search";
      } else {
        void api.releaseKeyboard();
        void win.hide();
      }
      return;
    }
    // AI mode: navigate actions, Enter/1-9 runs, Esc falls through to the hide above.
    if (mode === "ai") {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        aiSelected = Math.min(aiSelected + 1, aiActions.length - 1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        aiSelected = Math.max(aiSelected - 1, 0);
      } else if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        void runAi(aiActions[aiSelected]);
      } else if (/^[1-9]$/.test(e.key)) {
        e.preventDefault();
        void runAi(aiActions[Number(e.key) - 1]);
      }
      return;
    }
    // Inputs mode: Enter confirms the form (Tab keeps native focus cycling between fields).
    if (mode === "inputs" && e.key === "Enter") {
      const tag = (e.target as HTMLElement | null)?.tagName ?? "";
      if (/^(input|select)$/i.test(tag)) {
        e.preventDefault();
        void confirmInputs();
      }
      return;
    }
    const list = mode === "choose" ? candidates : filtered;
    const sel = mode === "choose" ? chooseSelected : selected;
    const setSel = mode === "choose" ? (v: number) => (chooseSelected = v) : (v: number) => (selected = v);
    if (mode === "search" || mode === "choose") {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSel(Math.min(sel + 1, Math.max(list.length - 1, 0)));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSel(Math.max(sel - 1, 0));
      } else if (e.key === "Enter" || e.key === "Tab") {
        // Tab = accept, same as Enter (documented in Settings → Shortcuts).
        e.preventDefault();
        const p = list[sel];
        if (p) void pick(p);
      }
    }
  }

  onMount(() => {
    let unlistenFocus: (() => void) | undefined;
    let unlistenChoose: (() => void) | undefined;
    let unlistenOffer: (() => void) | undefined;
    let unlistenOfferKey: (() => void) | undefined;
    let unlistenAi: (() => void) | undefined;
    let unlistenAiErr: (() => void) | undefined;
    let unlistenAiResult: (() => void) | undefined;
    let unlistenAiDelta: (() => void) | undefined;
    let unlistenAiCancelled: (() => void) | undefined;
    void (async () => {
      await load();
      await reset();
      unlistenFocus = await win.onFocusChanged(({ payload: focused }) => {
        if (focused) {
          void load();
          void reset();
        } else {
          mode = "search"; // reset mode when hidden
        }
      });
      unlistenChoose = await listen<string[]>("autotext-choose", (e) => {
        void onChoose(e.payload);
      });
      unlistenOffer = await listen<{ original: string; candidates: string[] }>(
        "spellcheck-offer",
        (e) => {
          void onOffer(e.payload);
        },
      );
      // Nav keys routed from the rdev listener (works regardless of X focus delivery).
      unlistenOfferKey = await listen<string>("offer-key", (e) => {
        handleOfferKey(e.payload);
      });
      unlistenAi = await listen<string>("ai-selection", (e) => {
        void onAiSelection(e.payload);
      });
      // Capture failed (nothing was under the selection, or the copy was rejected).
      // Still open the popup — it's the only feedback surface the user is looking at.
      unlistenAiErr = await listen<string>("ai-selection-error", (e) => {
        void onAiSelection(e.payload, true);
      });
      // run_ai_action succeeded: the window is already visible (Rust re-showed it);
      // swap to the result view. Rust holds the rewrite until ai_decide resolves it.
      // The payload carries the original selection alongside the rewrite (diff view).
      unlistenAiResult = await listen<{ result: string; original: string }>("ai-result", (e) => {
        aiResult = e.payload.result;
        aiOriginal = e.payload.original;
        streaming = false;
        mode = "airesult";
        void api.focusPopup();
      });
      // Streaming deltas (accumulated text from Rust): show them as they arrive.
      // The first delta swaps to the result view; decisions stay locked until the
      // ai-result event flips `streaming` off.
      unlistenAiDelta = await listen<string>("ai-delta", (e) => {
        aiResult = e.payload;
        streaming = true;
        if (mode !== "airesult") mode = "airesult";
      });
      // The user cancelled a stream: nothing to decide, stand down and hide
      // (focus returns to the target app — the selection was never touched).
      unlistenAiCancelled = await listen<void>("ai-cancelled", () => {
        streaming = false;
        busy = false;
        mode = "search";
        void win.hide();
      });
    })();
    // Sync return (not async) — onMount cleanup must be a plain function, not a Promise.
    return () => {
      unlistenFocus?.();
      unlistenChoose?.();
      unlistenOffer?.();
      unlistenOfferKey?.();
      unlistenAi?.();
      unlistenAiErr?.();
      unlistenAiResult?.();
      unlistenAiDelta?.();
      unlistenAiCancelled?.();
    };
  });
</script>

<svelte:window onkeydown={onkeydown} />

<div class="popup" class:fill={mode === "airesult"} bind:this={shell}>
  <!-- Drag grip: the always-there grab handle (every header is draggable too). -->
  <div class="grip" data-tauri-drag-region title="Drag to move">
    <span class="dots" data-tauri-drag-region>···</span>
  </div>
  {#key mode}
    <div class="mode" in:fly={{ y: 4, duration: 90 }}>
      {#if mode === "inputs" && current}
        <PopupInputs
          current={current}
          bind:formInputs
          {busy}
          onconfirm={confirmInputs}
          onback={() => (mode = "search")}
        />
      {:else if mode === "choose"}
        <PopupChoose candidates={candidates} bind:selected={chooseSelected} onpick={pick} />
      {:else if mode === "offer"}
        <PopupOffer
          original={offerOriginal}
          candidates={offerCandidates}
          bind:selected={offerSelected}
          preview={offerPreview}
          {busy}
          onaccept={acceptOffer}
          onignore={ignoreOffer}
        />
      {:else if mode === "ai"}
        <PopupAi
          actions={aiActions}
          selected={aiSelected}
          preview={aiPreview}
          {busy}
          onpick={runAi}
        />
      {:else if mode === "aiarg" && aiArgAction}
        <PopupAiArg
          actionName={aiArgAction.name}
          tokens={aiArgTokens}
          bind:params={aiParams}
          {busy}
          onrun={(p) => {
            const a = aiArgAction;
            aiArgAction = null;
            if (a) {
              const clean = Object.fromEntries(
                Object.entries(p).filter(([, v]) => v.trim() !== ""),
              );
              rememberParams(a.id, clean);
              void doRunAi(a.id, clean);
            }
          }}
          onback={() => {
            mode = "ai";
            aiArgAction = null;
          }}
        />
      {:else if mode === "airesult"}
        <PopupAiResult
          bind:this={aiResultView}
          result={aiResult}
          original={aiOriginal}
          {busy}
          {streaming}
          ondecide={decideAi}
        />
      {:else}
        <PopupSearch bind:this={search} bind:q {filtered} bind:selected onpick={pick} />
      {/if}
    </div>
  {/key}
  {#if error}<div class="error">{error}</div>{/if}
</div>

<style>
  .popup {
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    padding: 6px 8px 8px;
    gap: 6px;
  }
  /* airesult: fill the (fixed-size) window so the response area flexes to it. */
  .popup.fill {
    height: 100%;
  }
  .grip {
    flex: none;
    display: flex;
    justify-content: center;
    align-items: center;
    height: 10px;
    cursor: grab;
    user-select: none;
    border-radius: var(--radius-xs);
  }
  .grip:hover {
    background: var(--surface-2);
  }
  .grip:active {
    cursor: grabbing;
  }
  .dots {
    font-size: 10px;
    letter-spacing: 4px;
    color: var(--text-faint);
    line-height: 1;
    margin-right: -4px; /* optically center the dotted trio */
  }
  .grip:hover .dots {
    color: var(--accent);
  }
  .mode {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>
