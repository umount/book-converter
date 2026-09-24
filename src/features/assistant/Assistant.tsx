import { useEffect, useRef, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type { AssistantView, JobRef } from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";

export function Assistant({
  projectId,
  chapterId,
  t,
  onJob,
  refresh,
  registerFlush,
  beforeWork,
  onClose,
}: {
  onClose: () => void;
  beforeWork: () => Promise<void>;
  projectId: string;
  chapterId: string | null;
  t: T;
  onJob: (job: JobRef) => Promise<void>;
  refresh: () => Promise<void>;
  registerFlush: (flush: (() => Promise<void>) | null) => void;
}) {
  const [view, setView] = useState<AssistantView>({
    messages: [],
    proposals: [],
  });
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [autoRun, setAutoRun] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const active = useRef(true),
    lock = useRef(false);
  useEffect(() => {
    active.current = true;
    void projectApi
      .assistantView({ projectId })
      .then((v) => {
        if (active.current) setView(v);
      })
      .catch((e) => {
        if (active.current) setError(e);
      });
    return () => {
      active.current = false;
    };
  }, [projectId]);
  useEffect(() => {
    registerFlush(async () => {
      if (lock.current) throw new Error(t("processing"));
    });
    return () => registerFlush(null);
  });
  async function confirm(id: string, approved: boolean) {
    const job = await projectApi.assistantConfirm({
      projectId,
      proposalId: id,
      approved,
    });
    if (job) await onJob(job);
    if (approved) await refresh();
    if (active.current) setView(await projectApi.assistantView({ projectId }));
  }
  async function act(work: () => Promise<void>) {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError(null);
    try {
      await beforeWork();
      await work();
    } catch (e) {
      if (active.current) setError(e);
    } finally {
      lock.current = false;
      if (active.current) {
        setBusy(false);
        void projectApi
          .assistantView({ projectId })
          .then((v) => {
            if (active.current) setView(v);
          })
          .catch(() => {});
      }
    }
  }
  return (
    <section className="bc-tool bc-assistant">
      <header className="bc-assistant-header">
        <h2>{t("assistant")}</h2>
        <button
          type="button"
          aria-label={t("close")}
          title={t("close")}
          onClick={onClose}
        >
          ×
        </button>
      </header>
      <div className="bc-assistant-history">
        {view.messages.map((m) => (
          <article className={`bc-chat-message bc-chat-${m.role}`} key={m.id}>
            <strong>
              {m.role === "user"
                ? t("you")
                : m.role === "tool"
                  ? t("actionResult")
                  : t("assistant")}
            </strong>
            <p style={{ whiteSpace: "pre-wrap" }}>{m.text}</p>
          </article>
        ))}
        {view.proposals.map((p) => (
          <article className="bc-proposal" key={p.id}>
            <strong>
              {t(
                p.kind === "book_prompt"
                  ? "bookPrompt"
                  : p.kind === "chapter_prompt"
                    ? "instructions"
                    : p.kind === "glossary_term"
                      ? "glossary"
                      : p.kind === "translate_batch"
                        ? "translateBatch"
                        : "replace",
              )}
            </strong>
            {p.kind === "translate_batch" ? (
              <p>
                {t("batchCount")}: {p.after}
              </p>
            ) : (
              <div className="bc-proposal-diff">
                <div>
                  <small>{t("before")}</small>
                  <pre>{p.before || "—"}</pre>
                </div>
                <div>
                  <small>{t("after")}</small>
                  <pre>{p.after || "—"}</pre>
                </div>
              </div>
            )}
            <div className="bc-actions">
              <button
                className="primary"
                disabled={busy}
                onClick={() => void act(() => confirm(p.id, true))}
              >
                {t("apply")}
              </button>
              <button
                disabled={busy}
                onClick={() => void act(() => confirm(p.id, false))}
              >
                {t("decline")}
              </button>
            </div>
          </article>
        ))}
        {error != null && (
          <p className="bc-error" role="alert">
            {errorText(error, t)}
          </p>
        )}
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (busy || !draft.trim()) return;
          void act(async () => {
            const existing = new Set(view.proposals.map((p) => p.id));
            const result = await projectApi.assistantSend({
              projectId,
              chapterId,
              message: draft,
            });
            if (active.current) {
              setView(result);
              setDraft("");
            }
            if (autoRun)
              for (const p of result.proposals)
                if (!existing.has(p.id)) await confirm(p.id, true);
          });
        }}
      >
        <label>
          {t("assistantMessage")}
          <textarea
            rows={4}
            disabled={busy}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (
                e.key === "Enter" &&
                !e.shiftKey &&
                !e.nativeEvent.isComposing
              ) {
                e.preventDefault();
                if (!busy && draft.trim())
                  e.currentTarget.form?.requestSubmit();
              }
            }}
            placeholder={t("assistantExample")}
          />
        </label>
        <label className="bc-check">
          <input
            type="checkbox"
            checked={autoRun}
            disabled={busy}
            onChange={(e) => setAutoRun(e.target.checked)}
          />
          {t("assistantAutoRun")}
        </label>
        <div className="bc-actions">
          <small className="bc-hint">
            {busy ? t("processing") : t("assistantSendHint")}
          </small>
          {busy && (
            <button
              type="button"
              onClick={() =>
                void projectApi.assistantCancel({ projectId }).catch(setError)
              }
            >
              {t("cancel")}
            </button>
          )}
        </div>
      </form>
    </section>
  );
}
