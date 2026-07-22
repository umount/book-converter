import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { highlight } from "../types";
import type { ChapterRow, ChapterView } from "../types";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  chapters: ChapterRow[];
  chapterIdx: number | null;
  setChapterIdx: (idx: number) => void;
  chapter: ChapterView | null;
  chapterLoading: boolean;
  panes: { orig: boolean; transl: boolean };
  setPanes: Dispatch<SetStateAction<{ orig: boolean; transl: boolean }>>;
  hl: boolean;
  setHl: Dispatch<SetStateAction<boolean>>;
  sourceTerms: string[];
  targetTerms: string[];
  /** True while a translation job is running for this project. */
  translating: boolean;
  onTranslateChapter: (idx: number) => void;
  onSaveTranslation: (idx: number, title: string, body: string) => Promise<void>;
  /** Persist the chapter prompt, then optionally start a (re)translation. */
  onSaveChapterPrompt: (idx: number, prompt: string) => Promise<void>;
  /** Persist rolling summary + previous-chapter tail for this chapter's prompt. */
  onSaveChapterContext: (idx: number, summary: string, prevTail: string) => Promise<void>;
  onRetranslateWithPrompt: (idx: number, prompt: string) => Promise<void>;
};

export function Reader({
  t, chapters, chapterIdx, setChapterIdx, chapter, chapterLoading,
  panes, setPanes, hl, setHl, sourceTerms, targetTerms,
  translating, onTranslateChapter, onSaveTranslation,
  onSaveChapterPrompt, onSaveChapterContext, onRetranslateWithPrompt,
}: Props) {
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editBody, setEditBody] = useState("");
  const [saving, setSaving] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [promptDraft, setPromptDraft] = useState("");
  const [promptBusy, setPromptBusy] = useState(false);
  // Rolling context stays closed unless the user opens it.
  const [contextOpen, setContextOpen] = useState(false);
  const [summaryDraft, setSummaryDraft] = useState("");
  const [tailDraft, setTailDraft] = useState("");
  const [contextBusy, setContextBusy] = useState(false);

  // Sync drafts on chapter change. Context panel always closes (opt-in only).
  useEffect(() => {
    setEditing(false);
    setSaving(false);
    setPromptDraft(chapter?.user_prompt ?? "");
    setSummaryDraft(chapter?.rolling_summary ?? "");
    setTailDraft(chapter?.prev_tail ?? "");
    setContextOpen(false);
    if (chapter?.user_prompt) setPromptOpen(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chapterIdx, chapter?.user_prompt, chapter?.rolling_summary, chapter?.prev_tail]);

  function startEdit() {
    if (!chapter) return;
    setEditTitle(chapter.translated_title ?? "");
    setEditBody(chapter.translated ?? "");
    setEditing(true);
  }

  async function saveEdit() {
    if (chapterIdx == null) return;
    setSaving(true);
    try {
      await onSaveTranslation(chapterIdx, editTitle, editBody);
      setEditing(false);
    } finally {
      setSaving(false);
    }
  }

  async function savePromptOnly() {
    if (chapterIdx == null) return;
    setPromptBusy(true);
    try {
      await onSaveChapterPrompt(chapterIdx, promptDraft);
    } finally {
      setPromptBusy(false);
    }
  }

  async function saveContext() {
    if (chapterIdx == null) return;
    setContextBusy(true);
    try {
      await onSaveChapterContext(chapterIdx, summaryDraft, tailDraft);
    } finally {
      setContextBusy(false);
    }
  }

  async function regenerate() {
    if (chapterIdx == null || translating || editing) return;
    setPromptBusy(true);
    try {
      // Persist current prompt draft (may be empty) then re-run the model.
      await onRetranslateWithPrompt(chapterIdx, promptDraft);
    } finally {
      setPromptBusy(false);
    }
  }

  const hasTranslation = !!(chapter?.translated && chapter.status === "done");
  const canTranslate = !!chapter && chapterIdx != null && !translating;
  const canRegenerate = canTranslate && !editing && !promptBusy;
  const hasPrompt = !!(promptDraft.trim() || chapter?.user_prompt);
  const hasContext = !!(
    summaryDraft.trim() ||
    tailDraft.trim() ||
    chapter?.rolling_summary ||
    chapter?.prev_tail
  );

  return (
    <div className="reader">
      <div className="reader-toolbar">
        <button className="ghost" disabled={!chapters.length} onClick={() => {
          const i = chapters.findIndex((c) => c.idx === chapterIdx); if (i > 0) setChapterIdx(chapters[i - 1].idx);
        }}>‹</button>
        <select value={chapterIdx ?? ""} onChange={(e) => setChapterIdx(Number(e.target.value))}>
          {chapters.map((c) => {
            const label = (c.translated_title?.trim() || c.title).slice(0, 60);
            return (
              <option key={c.idx} value={c.idx}>
                {c.origin === "reference" ? "◆ " : c.origin === "manual" ? "✎ " : c.status === "done" ? "✓ " : "· "}
                {c.number != null ? `#${c.number} ` : ""}{label}
              </option>
            );
          })}
        </select>
        <button className="ghost" disabled={!chapters.length} onClick={() => {
          const i = chapters.findIndex((c) => c.idx === chapterIdx); if (i >= 0 && i < chapters.length - 1) setChapterIdx(chapters[i + 1].idx);
        }}>›</button>
        <div className="menu-spacer" />
        <button
          className={`chip ${promptOpen || hasPrompt ? "on" : ""}`}
          title={t("reader.promptTip")}
          disabled={!chapter}
          onClick={() => { setPromptOpen((o) => !o); if (!promptOpen) setContextOpen(false); }}
        >
          {t("reader.prompt")}{hasPrompt ? " ·" : ""}
        </button>
        <button
          className={`chip ${contextOpen ? "on" : ""}`}
          title={t("reader.contextTip")}
          disabled={!chapter}
          onClick={() => { setContextOpen((o) => !o); if (!contextOpen) setPromptOpen(false); }}
        >
          {t("reader.context")}{hasContext && !contextOpen ? " ·" : ""}
        </button>
        <label className="check"><input type="checkbox" checked={hl} onChange={(e) => setHl(e.target.checked)} /> {t("reader.highlight")}</label>
        <button className={`chip ${panes.orig ? "on" : ""}`} onClick={() => setPanes((p) => ({ ...p, orig: !p.orig }))}>{t("reader.original")}</button>
        <button className={`chip ${panes.transl ? "on" : ""}`} onClick={() => setPanes((p) => ({ ...p, transl: !p.transl }))}>{t("reader.translation")}</button>
      </div>

      {promptOpen && chapter && (
        <div className="chapter-prompt">
          <div className="chapter-prompt-head">
            <span className="chapter-prompt-title">{t("reader.promptTitle")}</span>
            <span className="muted">{t("reader.promptHint")}</span>
          </div>
          <textarea
            className="chapter-prompt-body"
            value={promptDraft}
            onChange={(e) => setPromptDraft(e.target.value)}
            placeholder={t("reader.promptPlaceholder")}
            disabled={translating || promptBusy}
          />
          <div className="chapter-prompt-actions">
            <button className="ghost" disabled={promptBusy || translating} onClick={() => void savePromptOnly()}>
              {t("reader.savePrompt")}
            </button>
            <button
              disabled={!canRegenerate}
              onClick={() => void regenerate()}
              title={t("reader.retranslateWithPromptTip")}
            >
              {translating || promptBusy
                ? t("reader.translating")
                : hasTranslation
                  ? t("reader.retranslateWithPrompt")
                  : t("reader.translateWithPrompt")}
            </button>
          </div>
        </div>
      )}

      {contextOpen && chapter && (
        <div className="chapter-prompt">
          <div className="chapter-prompt-head">
            <span className="chapter-prompt-title">{t("reader.contextTitle")}</span>
            <span className="muted">{t("reader.contextHint")}</span>
          </div>
          <label className="chapter-prompt-label">{t("reader.contextSummary")}</label>
          <textarea
            className="chapter-prompt-body chapter-prompt-body-lg"
            value={summaryDraft}
            onChange={(e) => setSummaryDraft(e.target.value)}
            placeholder={t("reader.contextSummaryPlaceholder")}
            disabled={translating || contextBusy}
          />
          <label className="chapter-prompt-label">{t("reader.contextTail")}</label>
          <textarea
            className="chapter-prompt-body"
            value={tailDraft}
            onChange={(e) => setTailDraft(e.target.value)}
            placeholder={t("reader.contextTailPlaceholder")}
            disabled={translating || contextBusy}
          />
          <div className="chapter-prompt-actions">
            <button className="ghost" disabled={contextBusy || translating} onClick={() => setContextOpen(false)}>
              {t("reader.contextClose")}
            </button>
            <button disabled={contextBusy || translating} onClick={() => void saveContext()}>
              {contextBusy ? t("reader.saving") : t("reader.saveContext")}
            </button>
            <button
              disabled={!canRegenerate || contextBusy}
              onClick={() => void (async () => {
                if (chapterIdx == null) return;
                setContextBusy(true);
                try {
                  await onSaveChapterContext(chapterIdx, summaryDraft, tailDraft);
                  await regenerate();
                } finally {
                  setContextBusy(false);
                }
              })()}
              title={t("reader.regenerateTip")}
            >
              {t("reader.saveContextAndRegenerate")}
            </button>
          </div>
        </div>
      )}

      <div className="panes">
        {panes.orig && (
          <div className="pane">
            <div className="pane-head">
              <span>{t("reader.original")} {chapter?.number != null && `· #${chapter.number}`}</span>
              <button className="icon" onClick={() => setPanes((p) => ({ ...p, orig: false }))}>×</button>
            </div>
            <div className="pane-body">
              {chapterLoading ? <div className="loading"><span className="spinner" /> {t("reader.loading")}</div> : <>
                <div className="chtitle">{chapter?.source_title}</div>
                <div className="chtext">{chapter ? (hl ? highlight(chapter.source, sourceTerms) : chapter.source) : ""}</div>
              </>}
            </div>
          </div>
        )}
        {panes.transl && (
          <div className="pane">
            <div className="pane-head">
              <span>{t("reader.translation")} {chapter?.status !== "done" && `· ${t("reader.notTranslated")}`}</span>
              {chapter?.origin === "reference" && <span className="ref-badge" title={t("reader.fromReferenceTip")}>{t("reader.fromReference")}</span>}
              {chapter?.origin === "manual" && <span className="ref-badge" title={t("reader.manualTip")}>{t("reader.manual")}</span>}
              <div className="menu-spacer" />
              {hasTranslation && !editing && (
                <>
                  <button className="ghost" onClick={startEdit}>{t("reader.edit")}</button>
                  <button
                    className="ghost"
                    disabled={!canRegenerate}
                    onClick={() => void regenerate()}
                    title={t("reader.regenerateTip")}
                  >
                    {promptBusy || translating ? t("reader.translating") : t("reader.regenerate")}
                  </button>
                </>
              )}
              {editing && (
                <>
                  <button className="ghost" disabled={saving} onClick={() => setEditing(false)}>{t("reader.cancelEdit")}</button>
                  <button disabled={saving} onClick={() => void saveEdit()}>{saving ? t("reader.saving") : t("reader.saveEdit")}</button>
                </>
              )}
              <button className="icon" onClick={() => setPanes((p) => ({ ...p, transl: false }))}>×</button>
            </div>
            <div className="pane-body">
              {chapterLoading ? <div className="loading"><span className="spinner" /> {t("reader.loading")}</div> : editing ? (
                <div className="ch-edit">
                  <input
                    className="ch-edit-title"
                    value={editTitle}
                    onChange={(e) => setEditTitle(e.target.value)}
                    placeholder={t("reader.editTitlePlaceholder")}
                  />
                  <textarea
                    className="ch-edit-body"
                    value={editBody}
                    onChange={(e) => setEditBody(e.target.value)}
                    placeholder={t("reader.editBodyPlaceholder")}
                  />
                </div>
              ) : <>
                <div className="chtitle">{chapter?.translated_title}</div>
                <div className="chtext">
                  {hasTranslation
                    ? (hl ? highlight(chapter!.translated!, targetTerms) : chapter!.translated)
                    : (
                      <div className="ch-empty">
                        <button
                          disabled={!canTranslate}
                          onClick={() => chapterIdx != null && onTranslateChapter(chapterIdx)}
                        >
                          {translating ? t("reader.translating") : t("reader.translateChapter")}
                        </button>
                      </div>
                    )}
                </div>
              </>}
            </div>
          </div>
        )}
        {!panes.orig && !panes.transl && <div className="muted" style={{ padding: 20 }}>{t("reader.bothClosed")}</div>}
      </div>
    </div>
  );
}
