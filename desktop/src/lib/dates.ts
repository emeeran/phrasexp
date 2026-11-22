// The app-wide display format: dd-mm-yyyy hh:mm (see also the date macro defaults).

/** ISO timestamp → "dd-mm-yyyy hh:mm" in local time; returns the raw string if unparseable. */
export function fmtDateTime(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getDate())}-${p(d.getMonth() + 1)}-${d.getFullYear()} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
