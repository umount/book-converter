import { useConfirm } from "../../shared/ui/useConfirm";
import { useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { assetUrl } from "../../shared/api/assets";
import { TITLE_DRAFT, type BookEditorSession } from "../../shared/state/editor";
import type { T } from "../../app/strings";
import { errorText } from "../../app/strings";
function AutoText({
  value,
  onChange,
  label,
}: {
  value: string;
  onChange: (v: string) => void;
  label: string;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const element = ref.current!;
    const resize = () => {
      element.style.height = "0px";
      element.style.height = `${Math.max(100, element.scrollHeight + 4)}px`;
    };
    resize();
    const observer = new ResizeObserver(() => {
      if (element.clientWidth !== width) {
        width = element.clientWidth;
        resize();
      }
    });
    let width = element.clientWidth;
    observer.observe(element);
    return () => observer.disconnect();
  }, [value]);
  return (
    <textarea
      ref={ref}
      className="bc-prose-editor"
      aria-label={label}
      spellCheck
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}
export function BookReader({
  session,
  t,
  focusBlock,
  onTranslateTitle,
  busy,
}: {
  session: BookEditorSession;
  focusBlock?: string | null;
  onTranslateTitle: () => void;
  busy: boolean;
  t: T;
}) {
  const confirmation = useConfirm(t);
  const state = useSyncExternalStore(session.subscribe, session.snapshot),
    { view } = state;
  useLayoutEffect(() => {
    if (focusBlock)
      document
        .getElementById(`book-block-${focusBlock}`)
        ?.scrollIntoView({ block: "center" });
  }, [focusBlock, view.chapter.id]);
  return (
    <div className="bc-reader">
      {confirmation.dialog}
      <div className="bc-reader-titles">
        <div>
          <h2>{view.chapter.title}</h2>
        </div>
        <div>
          {view.translation ? (
            <>
              <input
                className="bc-title-editor"
                aria-label={t("translatedChapterTitle")}
                value={state.drafts.get(TITLE_DRAFT) ?? view.translation.title}
                onChange={(e) => session.editTitle(e.target.value)}
              />
              <button
                className="bc-icon-button"
                aria-label={t("translateTitle")}
                title={t("translateTitle")}
                disabled={busy || state.saving}
                onClick={onTranslateTitle}
              >
                <svg
                  aria-hidden="true"
                  width="17"
                  height="17"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.7"
                >
                  <path d="M4 7h14m-4-4 4 4-4 4M20 17H6m4-4-4 4 4 4" />
                </svg>
              </button>
            </>
          ) : (
            <h2>—</h2>
          )}
        </div>
      </div>
      <div className="bc-reader-status">
        <div className="bc-translation-status">
          <p role="status">
            {t(
              view.status === "failed"
                ? "chapterFailed"
                : view.status === "in_progress"
                  ? "chapterInProgress"
                  : view.status === "done"
                    ? "chapterDone"
                    : view.status === "skipped"
                      ? "chapterSkipped"
                      : "chapterPending",
            )}
            {view.translation &&
              ` · ${t(view.translation.origin === "reference" ? "originReference" : view.translation.origin === "manual" ? "originManual" : "originModel")}`}
            {view.translation && view.translation.status !== "ready"
              ? ` · ${t("review")}`
              : ""}
            {view.translation && ` · ${state.saving ? t("saving") : state.drafts.size ? t("unsaved") : t("saved")}`}
          </p>
          {view.translationError && (
            <p role="alert">{errorText(view.translationError, t)}</p>
          )}
          {view.langIssues.length > 0 && (
            <p role="status">
              {t("languageIssues")}: {view.langIssues.join(", ")}
            </p>
          )}
          {state.error != null && (
            <div role="alert" className="bc-error bc-editor-error">
              <p>{errorText(state.error, t)}</p>
              <button onClick={() => void session.flush().catch(() => {})}>
                {t("retry")}
              </button>
              <button
                onClick={async () => {
                  if (
                    await confirmation.confirm({
                      title: t("reload"),
                      message: t("discardConfirm"),
                      action: t("reload"),
                      danger: true,
                    })
                  )
                    void session.discard().catch(() => {});
                }}
              >
                {t("reload")}
              </button>
            </div>
          )}
        </div>
      </div>
      {view.blocks.map((block, index) => (
        <section
          id={`book-block-${block.id}`}
          className={`bc-block-pair ${focusBlock === block.id ? "bc-found-block" : ""}`}
          key={block.id}
        >
          <div className="bc-source-block">
            <span className="bc-block-number" aria-hidden="true">
              {index + 1}
            </span>
            {block.content.kind === "image" ? (
              <img
                loading="lazy"
                src={assetUrl(session.projectId, block.content.asset_id)}
                alt={block.content.alt}
              />
            ) : (
              <p>{block.content.text}</p>
            )}
          </div>
          <div className="bc-target-block">
            {block.content.kind === "image" ? (
              <img
                loading="lazy"
                src={assetUrl(session.projectId, block.content.asset_id)}
                alt={block.content.alt}
              />
            ) : view.translation ? (
              <AutoText
                label={`${t("translation")} ${index + 1}`}
                value={state.drafts.get(block.id) ?? block.translatedText ?? ""}
                onChange={(text) => session.edit(block.id, text)}
              />
            ) : (
              <p className="bc-hint">{t("noTranslation")}</p>
            )}
          </div>
        </section>
      ))}
    </div>
  );
}
