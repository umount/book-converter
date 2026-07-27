import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { CallFn } from "../api";
import { LANGS, normalizeLang, type Lang } from "../i18n";
import { TRANSLATION_LANGS, type EffectiveConfig } from "../types";
import { SettingRow } from "./settings/SettingRow";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  lang: Lang;
  setLang: (l: Lang) => void;
  srcLang: string;
  tgtLang: string;
  onChangeSourceLang: (v: string) => void;
  onChangeTargetLang: (v: string) => void;
  /** Glossary highlighting in the reader (persisted preference). */
  highlight: boolean;
  onChangeHighlight: (v: boolean) => void;
  onClose: () => void;
  call: CallFn;
};

type SectionId = "interface" | "translation" | "model" | "advanced";
type Row = { id: string; section: SectionId; label: string; desc: string; lockedKey?: string; el: ReactNode };

export function Settings({
  t, lang, setLang, srcLang, tgtLang, onChangeSourceLang, onChangeTargetLang,
  highlight, onChangeHighlight, onClose, call,
}: Props) {
  const [query, setQuery] = useState("");
  const [section, setSection] = useState<SectionId>("interface");
  const [eff, setEff] = useState<EffectiveConfig | null>(null);
  const [model, setModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [temperature, setTemperature] = useState("");
  const [chunk, setChunk] = useState("");
  const [retries, setRetries] = useState("");

  useEffect(() => {
    (async () => {
      const c = await call<EffectiveConfig>("get_effective_config");
      if (c) {
        setEff(c);
        setModel(c.model);
        setBaseUrl(c.base_url);
        setTemperature(String(c.temperature));
        setChunk(String(c.max_chunk_chars));
        setRetries(String(c.max_retries));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const locked = (k: string) => !!eff?.env_locked.includes(k);
  const save = (key: string, value: string) => void call("set_setting", { key, value });

  const sections: { id: SectionId; label: string }[] = [
    { id: "interface", label: t("settings.sectionInterface") },
    { id: "translation", label: t("settings.sectionTranslation") },
    { id: "model", label: t("settings.sectionModel") },
    { id: "advanced", label: t("settings.sectionAdvanced") },
  ];

  const rows: Row[] = [
    {
      id: "lang", section: "interface", label: t("settings.language"), desc: t("settings.languageHint"),
      el: (
        <select value={lang} onChange={(e) => setLang(normalizeLang(e.target.value))}>
          {LANGS.map((l) => <option key={l.code} value={l.code}>{l.label}</option>)}
        </select>
      ),
    },
    {
      id: "highlight", section: "interface", label: t("settings.highlightTerms"), desc: t("settings.highlightTermsHint"),
      el: (
        <label className="check">
          <input type="checkbox" checked={highlight} onChange={(e) => onChangeHighlight(e.target.checked)} />
          {t("settings.highlightTermsOn")}
        </label>
      ),
    },
    {
      id: "src", section: "translation", label: t("settings.sourceLang"), desc: t("settings.translationHint"), lockedKey: "source_lang",
      el: (
        <select value={srcLang} disabled={locked("source_lang")} onChange={(e) => onChangeSourceLang(e.target.value)}>
          {TRANSLATION_LANGS.map((l) => <option key={l} value={l}>{l}</option>)}
        </select>
      ),
    },
    {
      id: "tgt", section: "translation", label: t("settings.targetLang"), desc: t("settings.translationHint"), lockedKey: "target_lang",
      el: (
        <select value={tgtLang} disabled={locked("target_lang")} onChange={(e) => onChangeTargetLang(e.target.value)}>
          {TRANSLATION_LANGS.map((l) => <option key={l} value={l}>{l}</option>)}
        </select>
      ),
    },
    {
      id: "model", section: "model", label: t("settings.model"), desc: t("settings.modelDesc"), lockedKey: "model",
      el: <input value={model} disabled={locked("model")} onChange={(e) => setModel(e.target.value)} onBlur={() => save("model", model)} placeholder="deepseek-chat" />,
    },
    {
      id: "baseUrl", section: "model", label: t("settings.baseUrl"), desc: t("settings.baseUrlDesc"), lockedKey: "base_url",
      el: <input value={baseUrl} disabled={locked("base_url")} onChange={(e) => setBaseUrl(e.target.value)} onBlur={() => save("base_url", baseUrl)} placeholder="https://api.deepseek.com" />,
    },
    {
      id: "temperature", section: "model", label: t("settings.temperature"), desc: t("settings.temperatureDesc"),
      el: <input type="number" step="0.1" min="0" max="2" value={temperature} onChange={(e) => setTemperature(e.target.value)} onBlur={() => save("temperature", temperature)} />,
    },
    {
      id: "apiKey", section: "model", label: t("settings.apiKey"), desc: t("settings.apiKeyDesc"),
      el: <span className={eff?.has_key ? "muted" : "setting-warn"}>{eff?.has_key ? t("settings.apiKeySet") : t("settings.apiKeyMissing")}</span>,
    },
    {
      id: "chunk", section: "advanced", label: t("settings.maxChunk"), desc: t("settings.maxChunkDesc"),
      el: <input type="number" min="500" step="500" value={chunk} onChange={(e) => setChunk(e.target.value)} onBlur={() => save("max_chunk_chars", chunk)} />,
    },
    {
      id: "retries", section: "advanced", label: t("settings.maxRetries"), desc: t("settings.maxRetriesDesc"),
      el: <input type="number" min="0" max="20" value={retries} onChange={(e) => setRetries(e.target.value)} onBlur={() => save("max_retries", retries)} />,
    },
  ];

  const q = query.trim().toLowerCase();
  const shown = useMemo(
    () => (q ? rows.filter((r) => r.label.toLowerCase().includes(q) || r.desc.toLowerCase().includes(q)) : rows.filter((r) => r.section === section)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [q, section, lang, srcLang, tgtLang, highlight, model, baseUrl, temperature, chunk, retries, eff],
  );

  return (
    <div className="settings-page">
      <div className="workhead settings-head">
        <div className="worktitle">{t("settings.title")}</div>
        <button className="ghost" onClick={onClose}>{t("settings.close")}</button>
      </div>

      <div className="settings-search">
        <input placeholder={t("settings.search")} value={query} onChange={(e) => setQuery(e.target.value)} />
      </div>

      <div className="settings-body">
        <nav className="settings-nav">
          {sections.map((s) => (
            <button key={s.id} className={!q && section === s.id ? "active" : ""} onClick={() => { setQuery(""); setSection(s.id); }}>
              {s.label}
            </button>
          ))}
        </nav>

        <div className="settings-content">
          {sections.map((s) => {
            const secRows = shown.filter((r) => r.section === s.id);
            if (secRows.length === 0) return null;
            return (
              <section key={s.id}>
                <div className="settings-section-title">{s.label}</div>
                {secRows.map((r) => (
                  <SettingRow key={r.id} label={r.label} desc={r.desc} locked={!!r.lockedKey && locked(r.lockedKey)} lockedNote={t("settings.envLocked")}>
                    {r.el}
                  </SettingRow>
                ))}
              </section>
            );
          })}
          {shown.length === 0 && <div className="empty">{t("settings.noMatches")}</div>}
        </div>
      </div>
    </div>
  );
}
