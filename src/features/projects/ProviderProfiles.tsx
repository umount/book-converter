import { useEffect, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type {
  ProviderEntry,
  ProjectDescriptor,
  ProjectSettingsView,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";

export function ProviderProfiles({
  project,
  t,
  defaults,
  onBusy,
}: {
  project: ProjectDescriptor | null;
  t: T;
  defaults: { model: string; base_url: string; context_window_tokens: number; max_output_tokens: number } | null;
  onBusy: (busy: boolean) => void;
}) {
  const [profiles, setProfiles] = useState<ProviderEntry[]>([]);
  const [draft, setDraft] = useState<ProviderEntry | null>(null);
  const [credential, setCredential] = useState("");
  const [settings, setSettings] = useState<ProjectSettingsView | null>(null);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<unknown>(null),
    [notice, setNotice] = useState("");
  useEffect(() => {
    let alive = true;
    void Promise.all([
      projectApi.profiles(),
      project
        ? projectApi.settings({ projectId: project.id })
        : Promise.resolve(null),
    ])
      .then(([p, s]) => {
        if (alive) {
          setProfiles(p);
          setSettings(s);
          if (!p.length && project?.kind !== "manga") setDraft({
            id: crypto.randomUUID(), name: "",
            baseUrl: defaults?.base_url ?? "", model: defaults?.model ?? "",
            temperature: 0.3, maxOutputTokens: defaults?.max_output_tokens ?? 4096, contextWindowTokens: defaults?.context_window_tokens ?? 32768, timeoutSeconds: 120,
            networkRetries: 2, hasKey: false, revision: "0",
          });
        }
      })
      .catch((e) => {
        if (alive) setError(e);
      });
    return () => {
      alive = false;
    };
  }, [project?.id]);
  async function act(work: () => Promise<void>) {
    setBusy(true);
    onBusy(true);
    setError(null);
    setNotice("");
    try {
      await work();
      setNotice(t("saved"));
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
      onBusy(false);
    }
  }
  const roles =
    project?.kind === "book"
      ? (["bookTranslationProfile", "assistantProfile"] as const)
      : ([
          "mangaRecognitionProfile",
          "mangaTranslationProfile",
          "assistantProfile",
        ] as const);
  return (
    <details className="bc-provider-profiles" open={project?.kind === "manga" ? true : undefined}>
      <summary>{t("providerProfiles")}</summary>
      {error != null && (
        <p className="bc-error" role="alert">
          {errorText(error, t)}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
      <fieldset disabled={busy}>
        <h3>{t("providerProfiles")}</h3>
        {profiles.length === 0 && project?.kind !== "manga" && <p className="bc-hint">{t("noProviderProfiles")}</p>}
        {project?.kind === "manga" && <p className="bc-hint">{t("mangaProfileSetupHint")}</p>}
        <label>
          {t("choose")}
          <select
            value={draft?.id ?? ""}
            onChange={(e) => {
              setDraft(profiles.find((p) => p.id === e.target.value) ?? null);
              setCredential("");
            }}
          >
            <option value="">—</option>
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <button
          onClick={() => {
            setCredential("");
            setDraft({
              id: crypto.randomUUID(),
              name: "",
              baseUrl: defaults?.base_url ?? "",
              model: defaults?.model ?? "",
              temperature: 0.3,
              maxOutputTokens: defaults?.max_output_tokens ?? 4096, contextWindowTokens: defaults?.context_window_tokens ?? 32768,
              timeoutSeconds: 120,
              networkRetries: 2,
              hasKey: false,
              revision: "0",
            });
          }}
        >
          {t("newProfile")}
        </button>
        {draft && (
          <>
            <label>
              {t("profileName")}
              <input
                autoFocus
                value={draft.name}
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              />
            </label>
            <label>
              {t("model")}
              <input
                value={draft.model}
                onChange={(e) => setDraft({ ...draft, model: e.target.value })}
              />
            </label>
            <label>
              {t("endpoint")}
              <input
                type="url"
                value={draft.baseUrl}
                onChange={(e) =>
                  setDraft({ ...draft, baseUrl: e.target.value })
                }
              />
            </label>
            <label>
              {t("apiKey")}
              <input
                type="password"
                autoComplete="off"
                value={credential}
                placeholder={t("keyHint")}
                onChange={(e) => setCredential(e.target.value)}
              />
            </label>
            <p className="bc-hint">
              {t(draft.hasKey ? "keyStored" : "keyMissing")}{" "}
              {t("profileKeyHint")}
            </p>
            <div className="bc-fields">
              <label>
                {t("temperature")}
                <input
                  type="number"
                  min={0}
                  max={2}
                  step={0.1}
                  value={draft.temperature}
                  onChange={(e) =>
                    setDraft({ ...draft, temperature: Number(e.target.value) })
                  }
                />
              </label>
              <label>
                {t("contextTokens")}
                <input type="number" min={1} value={draft.contextWindowTokens}
                  onChange={(e) => setDraft({ ...draft, contextWindowTokens: Number(e.target.value) })} />
              </label>
              <label>
                {t("outputTokens")}
                <input
                  type="number"
                  min={1}
                  value={draft.maxOutputTokens}
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      maxOutputTokens: Number(e.target.value),
                    })
                  }
                />
              </label>
            </div>
            <button
              className="primary"
              disabled={
                !draft.name.trim() ||
                !draft.model.trim() ||
                !draft.baseUrl.trim() ||
                !Number.isInteger(draft.contextWindowTokens) ||
                !Number.isInteger(draft.maxOutputTokens) ||
                draft.maxOutputTokens < 1 ||
                draft.contextWindowTokens <= draft.maxOutputTokens
              }
              onClick={() =>
                void act(async () => {
                  const saved = await projectApi.saveProfile({
                    profile: draft,
                    credential: credential.trim() || null,
                    expectedRevision: profiles.some((p) => p.id === draft.id)
                      ? draft.revision
                      : null,
                  });
                  setDraft(saved);
                  setCredential("");
                  setProfiles(await projectApi.profiles());
                })
              }
            >
              {t("saveProfile")}
            </button>
          </>
        )}
        {settings && project && (
          <>
            <h3>
              {t("projectProviders")} · {project.name}
            </h3>
            {roles.map((role) => (
              <label key={role}>
                {t(role)}
                <select
                  value={settings.choices[role] ?? ""}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      choices: {
                        ...settings.choices,
                        [role]: e.target.value || null,
                      },
                    })
                  }
                >
                  <option value="">{t("defaultProvider")}</option>
                  {profiles.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} · {p.model}
                    </option>
                  ))}
                  {settings.choices[role] &&
                    !profiles.some((p) => p.id === settings.choices[role]) && (
                      <option value={settings.choices[role]!}>
                        {t("missingProfile")}
                      </option>
                    )}
                </select>
              </label>
            ))}
            <button
              onClick={() =>
                void act(async () => {
                  const revision = await projectApi.updateSettings({
                    projectId: project.id,
                    choices: settings.choices,
                    expectedRevision: settings.revision,
                  });
                  setSettings({ ...settings, revision });
                })
              }
            >
              {t("applyRoles")}
            </button>
          </>
        )}
      </fieldset>
    </details>
  );
}
