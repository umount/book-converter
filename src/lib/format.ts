// Shared formatting helpers used across views.

/** Human-readable duration from seconds: "45s", "3m", "2h 10m". */
export function formatEta(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  if (m < 60) return r > 0 ? `${m}m ${r}s` : `${m}m`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  return rm > 0 ? `${h}h ${rm}m` : `${h}h`;
}

const LANG_ABBR: Record<string, string> = {
  Chinese: "zh", Russian: "ru", English: "en", Japanese: "ja", Korean: "ko",
  German: "de", French: "fr", Spanish: "es", Italian: "it", Portuguese: "pt",
};

/** Two-letter code for a language name understood by the model ("Chinese" -> "zh"). */
export const langAbbr = (l: string) => LANG_ABBR[l] ?? l.slice(0, 2).toLowerCase();
