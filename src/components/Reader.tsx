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
  onRetranslateWithPrompt: (idx: number, prompt: string) => Promise<void>;
};

export function Reader({
  t, chapters, chapterIdx, setChapterIdx, chapter, chapterLoading,
  panes, setPanes, hl, setHl, sourceTerms, targetTerms,
  translating, onTranslateChapter, onSaveTranslation,
  onSaveChapterPrompt, onRetranslateWithPrompt,
}: Props) {
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editBody, setEditBody] = useState("");
  const [saving, setSaving] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [promptDraft, setPromptDraft] = useState("");
  const [promptBusy, setPromptBusy] = useState(false);

  // Leave edit mode / sync prompt draft when switching chapters.
  useEffect(() => {
    setEditing(false);
    setSaving(false);
    setPromptDraft(chapter?.user_prompt ?? "");
    // Keep the panel open if the new chapter already has a note; otherwise leave as-is.
    if (chapter?.user_prompt) setPromptOpen(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chapterIdx, chapter?.user_prompt]);

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

  async function applyAndRetranslate() {
    if (chapterIdx == null || translating) return;
    setPromptBusy(true);
    try {
      await onRetranslateWithPrompt(chapterIdx, promptDraft);
    } finally {
      setPromptBusy(false);
    }
  }

  const hasTranslation = !!(chapter?.translated && chapter.status === "done");
  const canTranslate = !!chapter && chapterIdx != null && !translating;
  const hasPrompt = !!(promptDraft.trim() || chapter?.user_prompt);

  return (
    <div className="reader">
      <div className="reader-toolbar">
        <button className="ghost" disabled={!chapters.length} onClick={() => {
          const i = chapters.findIndex((c) => c.idx === chapterIdx); if (i > 0) setChapterIdx(chapters[i - 1].idx);
        }}>‹</button>
        <select value={chapterIdx ?? ""} onChange={(e) => setChapterIdx(Number(e.target.value))}>
          {chapters.map((c) => (
            <option key={c.idx} value={c.idx}>
              {c.origin === "reference" ? "◆ " : c.origin === "manual" ? "✎ " : c.status === "done" ? "✓ " : "· "}{c.title.slice(0, 60)}
            </option>
          ))}
        </select>
        <button className="ghost" disabled={!chapters.length} onClick={() => {
          const i = chapters.findIndex((c) => c.idx === chapterIdx); if (i >= 0 && i < chapters.length - 1) setChapterIdx(chapters[i + 1].idx);
        }}>›</button>
        <div className="menu-spacer" />
        <button
          className={`chip ${promptOpen || hasPrompt ? "on" : ""}`}
          title={t("reader.promptTip")}
          disabled={!chapter}
          onClick={() => setPromptOpen((o) => !o)}
        >
          {t("reader.prompt")}{hasPrompt ? " ·" : ""}
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
              disabled={!canTranslate || promptBusy}
              onClick={() => void applyAndRetranslate()}
              title={t("reader.retranslateWithPromptTip")}
            >
              {translating
                ? t("reader.translating")
                : hasTranslation
                  ? t("reader.retranslateWithPrompt")
                  : t("reader.translateWithPrompt")}
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
                <button className="ghost" disabled={translating} onClick={startEdit}>{t("reader.edit")}</button>
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
