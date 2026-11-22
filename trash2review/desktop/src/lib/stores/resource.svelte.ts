// Shared data-loading helper — the skeleton + $effect + try/catch dance that was
// re-implemented per component. Svelte-5 runes module: the returned object is
// deeply reactive, so `resource.data` / `resource.loaded` work in $derived/$effect.

export interface Resource<T> {
  data: T;
  loaded: boolean;
  loading: boolean;
  error: string | null;
  /** (Re)fetch. Safe to call repeatedly; concurrent calls serialize on `loading`. */
  load(): Promise<void>;
  /** Load once — no-op when data is already in (the usual $effect guard). */
  loadOnce(): void;
}

export function createResource<T>(loader: () => Promise<T>, initial: T): Resource<T> {
  let data = $state<T>(initial);
  let loaded = $state(false);
  let loading = $state(false);
  let error = $state<string | null>(null);

  return {
    get data() {
      return data;
    },
    set data(v: T) {
      data = v;
    },
    get loaded() {
      return loaded;
    },
    get loading() {
      return loading;
    },
    get error() {
      return error;
    },
    async load() {
      if (loading) return;
      loading = true;
      error = null;
      try {
        data = await loader();
        loaded = true;
      } catch (e) {
        error = String(e);
      } finally {
        loading = false;
      }
    },
    loadOnce() {
      if (!loaded && !loading) void this.load();
    },
  };
}
