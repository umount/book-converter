import { LANGS, normalizeLang, type Lang } from "../i18n";
import { TRANSLATION_LANGS } from "../types";
import { Panel } from "./Panel";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  collapsed: Record<string, boolean>;
  onToggle: (id: string) => void;
  lang: Lang;
  setLang: (l: Lang) => void;
  srcLang: string;
  tgtLang: string;
  onChangeSourceLang: (v: string) => void;
  onChangeTargetLang: (v: string) => void;
  onClose: () => void;
};

export function Settings({
  t, collapsed, onToggle, lang, setLang, srcLang, tgtLang,
  onChangeSourceLang, onChangeTargetLang, onClose,
}: Props) {
  return (
    <div className="settings-page">
      <div className="workhead settings-head">
        <div className="worktitle">{t("settings.title")}</div>
        <button className="ghost" onClick={onClose}>{t("settings.close")}</button>
      </div>
      <Panel id="settings-lang" title={t("settings.language")} collapsed={collapsed} onToggle={onToggle}>
        <div className="row">
          <select value={lang} onChange={(e) => setLang(normalizeLang(e.target.value))} style={{ minWidth: 160 }}>
            {LANGS.map((l) => <option key={l.code} value={l.code}>{l.label}</option>)}
          </select>
        </div>
        <div className="muted" style={{ marginTop: 6 }}>{t("settings.languageHint")}</div>
      </Panel>
      <Panel id="settings-translation" title={t("settings.translation")} collapsed={collapsed} onToggle={onToggle}>
        <div className="row">
          <label style={{ minWidth: 130 }}>{t("settings.sourceLang")}</label>
          <select value={srcLang} onChange={(e) => onChangeSourceLang(e.target.value)} style={{ minWidth: 160 }}>
            {TRANSLATION_LANGS.map((l) => <option key={l} value={l}>{l}</option>)}
          </select>
        </div>
        <div className="row" style={{ marginTop: 8 }}>
          <label style={{ minWidth: 130 }}>{t("settings.targetLang")}</label>
          <select value={tgtLang} onChange={(e) => onChangeTargetLang(e.target.value)} style={{ minWidth: 160 }}>
            {TRANSLATION_LANGS.map((l) => <option key={l} value={l}>{l}</option>)}
          </select>
        </div>
        <div className="muted" style={{ marginTop: 6 }}>{t("settings.translationHint")}</div>
      </Panel>
    </div>
  );
}
