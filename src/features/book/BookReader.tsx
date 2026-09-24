import { useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { assetUrl } from "../../shared/api/assets";
import type { BookEditorSession } from "../../shared/state/editor";
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
}: {
  session: BookEditorSession;
  t: T;
}) {
  const state = useSyncExternalStore(session.subscribe, session.snapshot),
    { view } = state;
  return (
    <div className="bc-reader">
      <div className="bc-reader-titles">
        <div>
          <span>{t("original")}</span>
          <h2>{view.chapter.title}</h2>
        </div>
        <div>
          <span>{t("translation")}</span>
          <h2>{view.translation?.title ?? "—"}</h2>
          <small role="status">
            {state.saving
              ? t("saving")
              : state.drafts.size
                ? t("unsaved")
                : t("saved")}
            {view.translation && view.translation.status !== "ready"
              ? ` · ${t("review")}`
              : ""}
          </small>
        </div>
      </div>
      {state.error != null && (
        <div role="alert" className="bc-error bc-editor-error">
          <p>{errorText(state.error, t)}</p>
          <button onClick={() => void session.flush().catch(() => {})}>
            {t("retry")}
          </button>
          <button
            onClick={() => {
              if (window.confirm(t("reload")))
                void session.discard().catch(() => {});
            }}
          >
            {t("reload")}
          </button>
        </div>
      )}
      {view.blocks.map((block, index) => (
        <section className="bc-block-pair" key={block.id}>
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
