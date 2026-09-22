import { useState } from "react";
import { TRANSLATION_LANGS } from "../types";
import { Modal } from "./common/Modal";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  name: string;
  source: string;
  target: string;
  detected: boolean;
  onConfirm: (source: string, target: string) => void;
  onCancel: () => void;
};

function listed(lang: string): string {
  return (TRANSLATION_LANGS as readonly string[]).includes(lang) ? lang : "English";
}

/**
 * Blocking dialog after a book is parsed: confirm the detected source language
 * and pick the target. Confirm is required; backdrop clicks do not dismiss it.
 */
export function LanguageSetup({ t, name, source, target, detected, onConfirm, onCancel }: Props) {
  const [src, setSrc] = useState(() => listed(source));
  const [tgt, setTgt] = useState(() => listed(target));

  return (
    <Modal className="lang-setup" onClose={onCancel} closeOnBackdrop={false} labelledBy="lang-setup-title">
      <div className="lang-setup-title" id="lang-setup-title">{t("langSetup.title")}</div>
      <p className="lang-setup-hint">{t("langSetup.hint", { name })}</p>

      <label className="lang-setup-field">
        <span>{t("langSetup.source")}</span>
        <select value={src} onChange={(e) => setSrc(e.target.value)} autoFocus>
          {TRANSLATION_LANGS.map((lang) => (
            <option key={lang} value={lang}>{lang}</option>
          ))}
        </select>
        <span className="lang-setup-note">
          {detected ? t("langSetup.sourceDetected") : t("langSetup.sourceFallback")}
        </span>
      </label>

      <label className="lang-setup-field">
        <span>{t("langSetup.target")}</span>
        <select value={tgt} onChange={(e) => setTgt(e.target.value)}>
          {TRANSLATION_LANGS.map((lang) => (
            <option key={lang} value={lang}>{lang}</option>
          ))}
        </select>
        <span className="lang-setup-note">{t("langSetup.targetHint")}</span>
      </label>

      <div className="lang-setup-actions">
        <button className="ghost" type="button" onClick={onCancel}>{t("langSetup.cancel")}</button>
        <button className="primary" type="button" onClick={() => onConfirm(src, tgt)}>
          {t("langSetup.confirm")}
        </button>
      </div>
    </Modal>
  );
}
