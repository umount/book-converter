import { save as saveFile } from "@tauri-apps/plugin-dialog";
import { ProviderProfiles } from "../features/projects/ProviderProfiles";
import type { ProjectDescriptor } from "../shared/contracts/generated";
import { useEffect, useState } from "react";
import { desktopInvoke as invoke } from "../shared/api/desktop";
import { Modal } from "../shared/ui/Modal";
import { ModelDownloads } from "../features/manga/ModelDownloads";
import { LANGS, type Lang } from "../i18n";
import { errorText, languages, languageName, type T } from "./strings";
type Config = {
  full_logging: boolean;
  log_directory: string;
  model: string;
  context_window_tokens: number;
  max_output_tokens: number;
  target_lang: string;
  base_url: string;
  has_key: boolean;
  env_locked: string[];
};
export function Settings({
  project,
  t,
  lang,
  setLang,
  onClose,
}: {
  project: ProjectDescriptor | null;
  t: T;
  lang: Lang;
  setLang: (v: Lang) => void;
  onClose: () => void;
}) {
  const [config, setConfig] = useState<Config | null>(null),
    [key, setKey] = useState(""),
    [busy, setBusy] = useState(false),
    [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    void invoke<Config>("get_effective_config")
      .then((v) => {
        if (alive) setConfig(v);
      })
      .catch((e) => {
        if (alive) setError(e);
      });
    return () => {
      alive = false;
    };
  }, []);
  async function save() {
    if (!config) return;
    setBusy(true);
    setError(null);
    try {
      for (const [k, value] of [
        ["full_logging", String(config.full_logging)],
        ["target_lang", config.target_lang],
        ["model", config.model],
        ["context_window_tokens", String(config.context_window_tokens)],
        ["max_output_tokens", String(config.max_output_tokens)],
        ["base_url", config.base_url],
      ])
        if (!config.env_locked.includes(k))
          await invoke("set_setting", { key: k, value });
      if (key.trim()) await invoke("set_api_key", { key });
      setKey("");
      onClose();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  async function exportLogs() {
    setBusy(true);
    setError(null);
    try {
      const destination = await saveFile({
        defaultPath: "book-converter-diagnostics.zip",
        filters: [{ name: "ZIP", extensions: ["zip"] }],
      });
      if (destination) await invoke("export_diagnostics", { destination });
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  async function setLogging(full: boolean) {
    if (!config) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("set_setting", { key: "full_logging", value: String(full) });
      setConfig({ ...config, full_logging: full });
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      closeLabel={t("close")}
      title={t("settings")}
      busy={busy}
      onClose={onClose}
      footer={
        <>
          <button disabled={busy} onClick={onClose}>
            {t("cancel")}
          </button>
          <button
            className="primary"
            disabled={busy || !config?.model.trim() || !config.base_url.trim() || !Number.isInteger(config.context_window_tokens) || !Number.isInteger(config.max_output_tokens) || config.max_output_tokens < 1 || config.context_window_tokens <= config.max_output_tokens}
            onClick={() => void save()}
          >
            {t("save")}
          </button>
        </>
      }
    >
      <label>
        {t("interfaceLanguage")}
        <select value={lang} onChange={(e) => setLang(e.target.value as Lang)}>
          {LANGS.map((v) => (
            <option key={v.code} value={v.code}>
              {v.label}
            </option>
          ))}
        </select>
      </label>
      {config && (
        <label>
          {t("defaultTargetLanguage")}
          <select
            value={config.target_lang}
            disabled={busy || config.env_locked.includes("target_lang")}
            onChange={(e) =>
              setConfig({ ...config, target_lang: e.target.value })
            }
          >
            {[...new Set([...languages, config.target_lang])].map((code) => (
              <option key={code} value={code}>
                {languageName(code, lang)}
              </option>
            ))}
          </select>
          <span className="bc-hint">{t("defaultTargetLanguageHint")}</span>
        </label>
      )}
      <p className="bc-hint">{t("providerHint")}</p>
      {config && (
        <>
          <label>
            {t("model")}
            <input
              value={config.model}
              disabled={busy || config.env_locked.includes("model")}
              onChange={(e) => setConfig({ ...config, model: e.target.value })}
            />
          </label>
          <label>
            {t("endpoint")}
            <input
              type="url"
              value={config.base_url}
              disabled={busy || config.env_locked.includes("base_url")}
              onChange={(e) =>
                setConfig({ ...config, base_url: e.target.value })
              }
            />
          </label>
          <label>
            {t("contextTokens")}
            <input type="number" min={1} value={config.context_window_tokens}
              onChange={(e) => setConfig({ ...config, context_window_tokens: Number(e.target.value) })} />
          </label>
          <label>
            {t("outputTokens")}
            <input type="number" min={1} value={config.max_output_tokens}
              onChange={(e) => setConfig({ ...config, max_output_tokens: Number(e.target.value) })} />
          </label>
          <label>
            {t("apiKey")}
            <input
              type="password"
              autoComplete="off"
              placeholder={t("keyHint")}
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
          </label>
          <p className="bc-hint">
            {t(config.has_key ? "keyStored" : "keyMissing")}
          </p>
        </>
      )}
      {error != null && (
        <p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>
      )}
      <ProviderProfiles
        project={project}
        t={t}
        defaults={config}
        onBusy={setBusy}
      />
      <ModelDownloads t={t} />
      {config && (
        <section>
          <h3>{t("diagnostics")}</h3>
          <label className="bc-check">
            <input type="checkbox" checked={config.full_logging} disabled={busy}
              onChange={(e) => void setLogging(e.target.checked)} />
            {t("fullLogging")}
          </label>
          <p className="bc-hint">{t("fullLoggingHint")}</p>
          <p className="bc-hint">{config.log_directory}</p>
          <button disabled={busy} onClick={() => void exportLogs()}>
            {t("exportDiagnostics")}
          </button>
        </section>
      )}
    </Modal>
  );
}
