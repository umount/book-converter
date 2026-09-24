export type Lang = "en" | "ru" | "zh";
export const LANGS: { code: Lang; label: string }[] = [
  { code: "en", label: "English" },
  { code: "ru", label: "Русский" },
  { code: "zh", label: "中文" },
];
export const LS_LANG = "bc.lang";
export function normalizeLang(value: string | null): Lang {
  return value === "ru" || value === "zh" ? value : "en";
}
