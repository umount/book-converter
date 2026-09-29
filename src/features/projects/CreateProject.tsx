import { useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { desktopInvoke } from "../../shared/api/desktop";
import { projectApi } from "../../shared/api/projects";
import type { ImportPreview, ImportProgress, ProjectDescriptor, } from "../../shared/contracts/generated";
import { Modal } from "../../shared/ui/Modal";
import { errorText, languages, languageName, type T } from "../../app/strings";
import type { Lang } from "../../i18n";
export function CreateProject({ t, lang, onClose, onCreated, }: {
    t: T;
    lang: Lang;
    onClose: () => void;
    onCreated: (project: ProjectDescriptor, metadata: boolean) => Promise<void>;
}) {
    const [preview, setPreview] = useState<ImportPreview | null>(null);
    const [name, setName] = useState(""), [source, setSource] = useState(""), [target, setTarget] = useState(lang === "en" ? "en" : lang);
    const [metadata, setMetadata] = useState(true), [busy, setBusy] = useState(false), [error, setError] = useState<unknown>(null);
    const [progress, setProgress] = useState<ImportProgress | null>(null);
    const targetTouched = useRef(false);
    const started = useRef(false);
    const [visible, setVisible] = useState(false);
    useEffect(() => {
        if (started.current) return;
        started.current = true;
        void chooseSource();
    }, []);
    useEffect(() => {
        let alive = true;
        void desktopInvoke<{
            target_lang: string;
        }>("get_effective_config")
            .then((config) => {
            if (alive && !targetTouched.current && config.target_lang)
                setTarget(config.target_lang);
        })
            .catch(() => { });
        return () => {
            alive = false;
        };
    }, []);
    async function chooseSource() {
        setError(null);
        setBusy(true);
        try {
            const path = await open({
                multiple: false,
                filters: [
                        {
                            name: t("book"),
                            extensions: ["txt", "fb2", "epub", "pdf", "zip"],
                        },
                    ],
            });
            if (typeof path !== "string") {
                if (!preview) onClose();
                return;
            }
            setVisible(true);
            if (preview) {
                await projectApi.cancelImport({ importId: preview.importId });
                setPreview(null);
            }
            setProgress({ stage: "scanning", completed: 0, total: null });
            const channel = new Channel<ImportProgress>();
            channel.onmessage = setProgress;
            const next = await projectApi.inspectSource({ kind: "book", path }, channel);
            setPreview(next);
            setName(next.suggestedName);
            setSource(next.detectedLanguage ?? "");
        }
        catch (e) {
            setVisible(true);
            setError(e);
        }
        finally {
            setBusy(false);
            setProgress(null);
        }
    }
    async function cancel() {
        setBusy(true);
        try {
            if (preview)
                await projectApi.cancelImport({ importId: preview.importId });
            onClose();
        }
        catch (e) {
            setVisible(true);
            setError(e);
        }
        finally {
            setBusy(false);
            setProgress(null);
        }
    }
    async function create() {
        if (!preview || !source || !target || !name.trim())
            return;
        setBusy(true);
        setProgress({ stage: "creating", completed: 0, total: null });
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
            await onCreated(project, metadata);
        }
        catch (e) {
            setVisible(true);
            setError(e);
        }
        finally {
            setBusy(false);
            setProgress(null);
        }
    }
    if (!visible) return null;
    return (<Modal closeLabel={t("close")} title={t("importBookSettings")} onClose={() => void cancel()} busy={busy} footer={<>
          <button disabled={busy} onClick={() => void cancel()}>
            {t("cancel")}
          </button>
          <button className="primary" disabled={busy || !preview || !name.trim() || !source || !target} onClick={() => void create()}>
            {t("openBook")}
          </button>
        </>}>
      {busy && progress && (<div className="bc-import-progress" role="status" aria-live="polite">
          <strong>
            {t(progress.stage === "finalizing"
                    ? "importFinalizing"
                    : progress.stage === "creating"
                        ? "importCreating"
                        : "importScanning")}
          </strong>
          <progress aria-label={t("importProgress")} value={progress.total ? progress.completed : undefined} max={progress.total || undefined}/>
          {progress.total != null && (<span>
              {progress.completed} / {progress.total} ·{" "}
              {Math.floor((progress.completed * 100) / progress.total)}%
            </span>)}
          <span className="bc-hint">{t("importWorking")}</span>
        </div>)}

      <button disabled={busy} onClick={() => void chooseSource()}>
        {busy ? t("loading") : t("chooseBook")}
      </button>
      {preview && (<>
          <p className="bc-file">{preview.source.displayName}</p>
          <label>
            {t("name")}
            <input autoFocus value={name} disabled={busy} onChange={(e) => setName(e.target.value)}/>
          </label>
          <div className="bc-fields">
            {([
                { value: source, set: setSource, label: "sourceLanguage" },
                {
                    value: target,
                    set: (value: string) => {
                        targetTouched.current = true;
                        setTarget(value);
                    },
                    label: "targetLanguage",
                },
            ] as const).map((field) => (<label key={field.label}>
                {t(field.label)}
                <select value={field.value} disabled={busy} onChange={(e) => field.set(e.target.value)}>
                  <option value="">{t("choose")}</option>
                  {[
                    ...new Set([...languages, source, target].filter(Boolean)),
                ].map((code) => (<option key={code} value={code}>
                      {languageName(code, lang)}
                    </option>))}
                </select>
              </label>))}
          </div>
          <p className="bc-hint">{t("fixedLanguages")}</p>
          {preview.warnings.length > 0 && (<p className="bc-warning">{t("importWarning")}</p>)}
          {(<label className="bc-check">
              <input type="checkbox" checked={metadata} disabled={busy} onChange={(e) => setMetadata(e.target.checked)}/>
              {t("metadataAfterCreate")}
            </label>)}
        </>)}
      {error != null && (<p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>)}
    </Modal>);
}
