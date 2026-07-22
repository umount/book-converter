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
};

export function Reader({
  t, chapters, chapterIdx, setChapterIdx, chapter, chapterLoading,
  panes, setPanes, hl, setHl, sourceTerms, targetTerms,
  translating, onTranslateChapter, onSaveTranslation,
}: Props) {
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editBody, setEditBody] = useState("");
  const [saving, setSaving] = useState(false);

  // Leave edit mode when switching chapters.
  useEffect(() => {
    setEditing(false);
    setSaving(false);
  }, [chapterIdx]);

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

  const hasTranslation = !!(chapter?.translated && chapter.status === "done");
  const canTranslate = !!chapter && chapter.status !== "done" && chapterIdx != null && !translating;

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
        <label className="check"><input type="checkbox" checked={hl} onChange={(e) => setHl(e.target.checked)} /> {t("reader.highlight")}</label>
        <button className={`chip ${panes.orig ? "on" : ""}`} onClick={() => setPanes((p) => ({ ...p, orig: !p.orig }))}>{t("reader.original")}</button>
        <button className={`chip ${panes.transl ? "on" : ""}`} onClick={() => setPanes((p) => ({ ...p, transl: !p.transl }))}>{t("reader.translation")}</button>
      </div>

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
