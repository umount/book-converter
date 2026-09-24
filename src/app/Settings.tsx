import { useEffect, useState } from "react";
import { desktopInvoke as invoke } from "../shared/api/desktop";
import { Modal } from "../shared/ui/Modal";
import { LANGS, type Lang } from "../i18n";
import { errorText, type T } from "./strings";
type Config = {
  model: string;
  base_url: string;
  has_key: boolean;
  env_locked: string[];
};
export function Settings({
  t,
  lang,
  setLang,
  onClose,
}: {
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
        ["model", config.model],
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
  return (
    <Modal
      closeLabel={t("close")}
      title={t("settings")}
      busy={busy}
      onClose={onClose}
    >
      <div className="bc-dialog-body">
        <label>
          {t("interfaceLanguage")}
          <select
            value={lang}
            onChange={(e) => setLang(e.target.value as Lang)}
          >
            {LANGS.map((v) => (
              <option key={v.code} value={v.code}>
                {v.label}
              </option>
            ))}
          </select>
        </label>
        <p className="bc-hint">{t("providerHint")}</p>
        {config && (
          <>
            <label>
              {t("model")}
              <input
                value={config.model}
                disabled={busy || config.env_locked.includes("model")}
                onChange={(e) =>
                  setConfig({ ...config, model: e.target.value })
                }
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
      </div>
      <footer>
        <button disabled={busy} onClick={onClose}>
          {t("cancel")}
        </button>
        <button
          className="primary"
          disabled={busy || !config?.model.trim() || !config.base_url.trim()}
          onClick={() => void save()}
        >
          {t("save")}
        </button>
      </footer>
    </Modal>
  );
}
