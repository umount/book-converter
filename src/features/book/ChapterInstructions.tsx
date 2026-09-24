import { useEffect, useRef, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type { BookEditorSession } from "../../shared/state/editor";
import { Modal } from "../../shared/ui/Modal";
import { errorText, type T } from "../../app/strings";
export function ChapterInstructions({
  session,
  close,
  registerFlush,
  t,
}: {
  session: BookEditorSession;
  close: () => void;
  registerFlush: (flush: (() => Promise<void>) | null) => void;
  t: T;
}) {
  const initial = useRef(session.snapshot().view);
  const [text, setText] = useState(initial.current.instructions);
  const saved = useRef(initial.current.instructions);
  const revision = useRef(initial.current.chapter.revision);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<unknown>(null);
  const pending = useRef<Promise<void> | null>(null);
  function flush() {
    if (pending.current) return pending.current;
    if (text === saved.current) return Promise.resolve();
    setBusy(true);
    setError(null);
    pending.current = (async () => {
      await session.flush();
      await projectApi.updateInstructions({
        projectId: session.projectId,
        chapterId: initial.current.chapter.id,
        instructions: text,
        expectedRevision: revision.current,
      });
      saved.current = text;
      await session.refresh();
      revision.current = session.snapshot().view.chapter.revision;
    })().finally(() => {
      pending.current = null;
      setBusy(false);
    });
    return pending.current;
  }
  useEffect(() => {
    registerFlush(flush);
    return () => registerFlush(null);
  });
  async function finish() {
    try {
      await flush();
      close();
    } catch (e) {
      setError(e);
    }
  }
  return (
    <Modal
      title={t("instructions")}
      closeLabel={t("close")}
      onClose={() => void finish()}
      busy={busy}
      footer={
        <>
          <button disabled={busy} onClick={close}>
            {t("cancel")}
          </button>
          <button
            className="primary"
            disabled={busy}
            onClick={() => void finish()}
          >
            {t("save")}
          </button>
        </>
      }
    >
      <p className="bc-hint">{t("instructionHint")}</p>
      <textarea
        aria-label={t("instructions")}
        rows={7}
        value={text}
        disabled={busy}
        onChange={(e) => setText(e.target.value)}
      />
      {error != null && (
        <p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>
      )}
    </Modal>
  );
}
