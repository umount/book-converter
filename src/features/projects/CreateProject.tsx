import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { desktopInvoke } from "../../shared/api/desktop";
import { projectApi } from "../../shared/api/projects";
import type {
  ImportPreview,
  ProjectDescriptor,
  ProjectKind,
} from "../../shared/contracts/generated";
import { Modal } from "../../shared/ui/Modal";
import { errorText, languages, languageName, type T } from "../../app/strings";
import type { Lang } from "../../i18n";
export function CreateProject({
  t,
  lang,
  onClose,
  onCreated,
}: {
  t: T;
  lang: Lang;
  onClose: () => void;
  onCreated: (project: ProjectDescriptor, metadata: boolean) => Promise<void>;
}) {
  const [kind, setKind] = useState<ProjectKind>("book"),
    [preview, setPreview] = useState<ImportPreview | null>(null);
  const [name, setName] = useState(""),
    [source, setSource] = useState(""),
    [target, setTarget] = useState(lang === "en" ? "en" : lang);
  const [metadata, setMetadata] = useState(true),
    [busy, setBusy] = useState(false),
    [error, setError] = useState<unknown>(null);
  const targetTouched = useRef(false);
  useEffect(() => {
    let alive = true;
    void desktopInvoke<{ target_lang: string }>("get_effective_config")
      .then((config) => {
        if (alive && !targetTouched.current && config.target_lang)
          setTarget(config.target_lang);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  async function chooseSource(directory = false) {
    setError(null);
    setBusy(true);
    try {
      const path = await open({
        multiple: false,
        directory,
        filters: directory
          ? undefined
          : [
              {
                name: t(kind),
                extensions:
                  kind === "book"
                    ? ["txt", "fb2", "epub", "pdf", "zip"]
                    : ["cbz", "zip"],
              },
            ],
      });
      if (typeof path !== "string") return;
      if (preview) {
        await projectApi.cancelImport({ importId: preview.importId });
        setPreview(null);
      }
      const next = await projectApi.inspectSource({ kind, path });
      setPreview(next);
      setName(next.suggestedName);
      setSource(next.detectedLanguage ?? "");
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  async function cancel() {
    setBusy(true);
    try {
      if (preview)
        await projectApi.cancelImport({ importId: preview.importId });
      onClose();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  async function create() {
    if (!preview || !source || !target || !name.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const project = await projectApi.create({
        importId: preview.importId,
        choices: {
          name: name.trim(),
          languages: { source, target },
          processingProfileId: null,
        },
      });
      await onCreated(project, kind === "book" && metadata);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      closeLabel={t("close")}
      title={t("newProject")}
      onClose={() => void cancel()}
      busy={busy}
      footer={
        <>
          <button disabled={busy} onClick={() => void cancel()}>
            {t("cancel")}
          </button>
          <button
            className="primary"
            disabled={busy || !preview || !name.trim() || !source || !target}
            onClick={() => void create()}
          >
            {t("create")}
          </button>
        </>
      }
    >
      {!preview && (
        <>
          <p>{t("chooseKind")}</p>
          <div className="bc-kind-options">
            {(["book", "manga"] as const).map((value) => (
              <button
                key={value}
                aria-pressed={kind === value}
                disabled={busy}
                onClick={() => setKind(value)}
              >
                <strong>{t(value)}</strong>
                <span>
                  {t(value === "book" ? "bookFormats" : "mangaFormats")}
                </span>
              </button>
            ))}
          </div>
        </>
      )}
      <button disabled={busy} onClick={() => void chooseSource()}>
        {busy ? t("loading") : t("chooseSource")}
      </button>
      {kind === "manga" && (
        <button disabled={busy} onClick={() => void chooseSource(true)}>
          {t("chooseFolder")}
        </button>
      )}
      {preview && (
        <>
          <p className="bc-file">{preview.source.displayName}</p>
          <label>
            {t("name")}
            <input
              autoFocus
              value={name}
              disabled={busy}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <div className="bc-fields">
            {(
              [
                { value: source, set: setSource, label: "sourceLanguage" },
                {
                  value: target,
                  set: (value: string) => {
                    targetTouched.current = true;
                    setTarget(value);
                  },
                  label: "targetLanguage",
                },
              ] as const
            ).map((field) => (
              <label key={field.label}>
                {t(field.label)}
                <select
                  value={field.value}
                  disabled={busy}
                  onChange={(e) => field.set(e.target.value)}
                >
                  <option value="">{t("choose")}</option>
                  {[
                    ...new Set([...languages, source, target].filter(Boolean)),
                  ].map((code) => (
                    <option key={code} value={code}>
                      {languageName(code, lang)}
                    </option>
                  ))}
                </select>
              </label>
            ))}
          </div>
          <p className="bc-hint">{t("fixedLanguages")}</p>
          {preview.warnings.length > 0 && (
            <p className="bc-warning">{t("importWarning")}</p>
          )}
          {kind === "book" && (
            <label className="bc-check">
              <input
                type="checkbox"
                checked={metadata}
                disabled={busy}
                onChange={(e) => setMetadata(e.target.checked)}
              />
              {t("metadataAfterCreate")}
            </label>
          )}
        </>
      )}
      {error != null && (
        <p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>
      )}
    </Modal>
  );
}
