import { useEffect, useMemo, useState, type Dispatch, type SetStateAction } from "react";
import { tokenizeLines } from "../lib/highlight";
import type { ChapterRow, ChapterView, Term } from "../types";
import { EditorSurface } from "./reader/EditorSurface";
import { TermPopover } from "./reader/TermPopover";

type PaneState = { orig: boolean; transl: boolean };
type PaneMode = "split" | "orig" | "transl";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  chapters: ChapterRow[];
  chapterIdx: number | null;
  setChapterIdx: (idx: number) => void;
  chapter: ChapterView | null;
  chapterLoading: boolean;
  panes: PaneState;
  setPanes: Dispatch<SetStateAction<PaneState>>;
  hl: boolean;
  setHl: Dispatch<SetStateAction<boolean>>;
  /** Full glossary (source, target, kind) for highlighting + the term popover. */
  terms: Term[];
  /** Jump to a term's entry in the glossary view. */
  onOpenGlossaryTerm: (source: string) => void;
  /** True while a translation job is running for this project. */
  translating: boolean;
  onTranslateChapter: (idx: number) => void;
  onSaveTranslation: (idx: number, title: string, body: string) => Promise<void>;
  onSaveChapterPrompt: (idx: number, prompt: string) => Promise<void>;
  onSaveChapterContext: (idx: number, summary: string, prevTail: string) => Promise<void>;
  onRetranslateWithPrompt: (idx: number, prompt: string) => Promise<void>;
};

export function Reader({
  t, chapters, chapterIdx, setChapterIdx, chapter, chapterLoading,
  panes, setPanes, hl, setHl, terms, onOpenGlossaryTerm,
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
  const [contextOpen, setContextOpen] = useState(false);
  const [summaryDraft, setSummaryDraft] = useState("");
  const [tailDraft, setTailDraft] = useState("");
  const [contextBusy, setContextBusy] = useState(false);

  // Glossary occurrence highlight: the active term key + the popover it opened.
  const [activeTerm, setActiveTerm] = useState<string | null>(null);
  const [popover, setPopover] = useState<{ term: Term; rect: DOMRect } | null>(null);

  // Sync drafts on chapter change. Context panel always closes (opt-in only).
  useEffect(() => {
    setEditing(false);
    setSaving(false);
    setPromptDraft(chapter?.user_prompt ?? "");
    setSummaryDraft(chapter?.rolling_summary ?? "");
    setTailDraft(chapter?.prev_tail ?? "");
    setContextOpen(false);
    setActiveTerm(null);
    setPopover(null);
    if (chapter?.user_prompt) setPromptOpen(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chapterIdx, chapter?.user_prompt, chapter?.rolling_summary, chapter?.prev_tail]);

  const termMap = useMemo(() => new Map(terms.map((tm) => [tm.source, tm])), [terms]);
  const srcMatches = useMemo(() => terms.map((tm) => ({ match: tm.source, key: tm.source })), [terms]);
  const tgtMatches = useMemo(() => terms.map((tm) => ({ match: tm.target, key: tm.source })), [terms]);
  const sourceLines = useMemo(
    () => tokenizeLines(chapter?.source ?? "", hl ? srcMatches : []),
    [chapter?.source, srcMatches, hl],
  );
  const translLines = useMemo(
    () => tokenizeLines(chapter?.translated ?? "", hl ? tgtMatches : []),
    [chapter?.translated, tgtMatches, hl],
  );

  function onTermClick(key: string, rect: DOMRect) {
    const term = termMap.get(key);
    if (!term) return;
    setActiveTerm(key);
    setPopover({ term, rect });
  }
  function closePopover() {
    setPopover(null);
    setActiveTerm(null);
  }

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
      await onRetranslateWithPrompt(chapterIdx, promptDraft);
    } finally {
      setPromptBusy(false);
    }
  }

  const pos = chapters.findIndex((c) => c.idx === chapterIdx);
  const gotoRel = (d: number) => {
    const j = pos + d;
    if (pos >= 0 && j >= 0 && j < chapters.length) setChapterIdx(chapters[j].idx);
  };

  const mode: PaneMode = panes.orig && panes.transl ? "split" : panes.transl ? "transl" : "orig";
  const setMode = (m: PaneMode) =>
    setPanes(m === "split" ? { orig: true, transl: true } : m === "orig" ? { orig: true, transl: false } : { orig: false, transl: true });

  const hasTranslation = !!(chapter?.translated && chapter.status === "done");
  const canTranslate = !!chapter && chapterIdx != null && !translating;
  const canRegenerate = canTranslate && !editing && !promptBusy;
  const hasPrompt = !!(promptDraft.trim() || chapter?.user_prompt);
  const hasContext = !!(summaryDraft.trim() || tailDraft.trim() || chapter?.rolling_summary || chapter?.prev_tail);
  const headTitle = chapter ? `${chapter.number != null ? `#${chapter.number} ` : ""}${chapter.translated_title?.trim() || chapter.source_title}` : "";

  return (
    <div className="reader">
      <div className="reader-toolbar">
        <button className="ghost" disabled={pos <= 0} onClick={() => gotoRel(-1)} title={t("reader.prev")}>‹</button>
        <button className="ghost" disabled={pos < 0 || pos >= chapters.length - 1} onClick={() => gotoRel(1)} title={t("reader.next")}>›</button>
        <div className="reader-chtitle" title={headTitle}>{headTitle}</div>
        <div className="menu-spacer" />
        <div className="pane-modes">
          <button className={`chip ${mode === "split" ? "on" : ""}`} onClick={() => setMode("split")}>{t("reader.paneSplit")}</button>
          <button className={`chip ${mode === "orig" ? "on" : ""}`} onClick={() => setMode("orig")}>{t("reader.original")}</button>
          <button className={`chip ${mode === "transl" ? "on" : ""}`} onClick={() => setMode("transl")}>{t("reader.translation")}</button>
        </div>
        <label className="check"><input type="checkbox" checked={hl} onChange={(e) => setHl(e.target.checked)} /> {t("reader.highlight")}</label>
        <button className={`chip ${promptOpen || hasPrompt ? "on" : ""}`} title={t("reader.promptTip")} disabled={!chapter}
          onClick={() => { setPromptOpen((o) => !o); if (!promptOpen) setContextOpen(false); }}>
          {t("reader.prompt")}{hasPrompt ? " ·" : ""}
        </button>
        <button className={`chip ${contextOpen ? "on" : ""}`} title={t("reader.contextTip")} disabled={!chapter}
          onClick={() => { setContextOpen((o) => !o); if (!contextOpen) setPromptOpen(false); }}>
          {t("reader.context")}{hasContext && !contextOpen ? " ·" : ""}
        </button>
      </div>

      {promptOpen && chapter && (
        <div className="chapter-prompt">
          <div className="chapter-prompt-head">
            <span className="chapter-prompt-title">{t("reader.promptTitle")}</span>
            <span className="muted">{t("reader.promptHint")}</span>
          </div>
          <textarea className="chapter-prompt-body" value={promptDraft} onChange={(e) => setPromptDraft(e.target.value)}
            placeholder={t("reader.promptPlaceholder")} disabled={translating || promptBusy} />
          <div className="chapter-prompt-actions">
            <button className="ghost" disabled={promptBusy || translating} onClick={() => void savePromptOnly()}>{t("reader.savePrompt")}</button>
            <button disabled={!canRegenerate} onClick={() => void regenerate()} title={t("reader.retranslateWithPromptTip")}>
              {translating || promptBusy ? t("reader.translating") : hasTranslation ? t("reader.retranslateWithPrompt") : t("reader.translateWithPrompt")}
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
          <textarea className="chapter-prompt-body chapter-prompt-body-lg" value={summaryDraft} onChange={(e) => setSummaryDraft(e.target.value)}
            placeholder={t("reader.contextSummaryPlaceholder")} disabled={translating || contextBusy} />
          <label className="chapter-prompt-label">{t("reader.contextTail")}</label>
          <textarea className="chapter-prompt-body" value={tailDraft} onChange={(e) => setTailDraft(e.target.value)}
            placeholder={t("reader.contextTailPlaceholder")} disabled={translating || contextBusy} />
          <div className="chapter-prompt-actions">
            <button className="ghost" disabled={contextBusy || translating} onClick={() => setContextOpen(false)}>{t("reader.contextClose")}</button>
            <button disabled={contextBusy || translating} onClick={() => void saveContext()}>{contextBusy ? t("reader.saving") : t("reader.saveContext")}</button>
            <button disabled={!canRegenerate || contextBusy} title={t("reader.regenerateTip")}
              onClick={() => void (async () => {
                if (chapterIdx == null) return;
                setContextBusy(true);
                try { await onSaveChapterContext(chapterIdx, summaryDraft, tailDraft); await regenerate(); }
                finally { setContextBusy(false); }
              })()}>
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
                <EditorSurface lines={sourceLines} activeKey={activeTerm} onTermClick={onTermClick} />
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
                  <button className="ghost" disabled={!canRegenerate} onClick={() => void regenerate()} title={t("reader.regenerateTip")}>
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
                  <input className="ch-edit-title" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} placeholder={t("reader.editTitlePlaceholder")} />
                  <textarea className="ch-edit-body" value={editBody} onChange={(e) => setEditBody(e.target.value)} placeholder={t("reader.editBodyPlaceholder")} />
                </div>
              ) : <>
                <div className="chtitle">{chapter?.translated_title}</div>
                {hasTranslation ? (
                  <EditorSurface lines={translLines} activeKey={activeTerm} onTermClick={onTermClick} />
                ) : (
                  <div className="ch-empty">
                    <button disabled={!canTranslate} onClick={() => chapterIdx != null && onTranslateChapter(chapterIdx)}>
                      {translating ? t("reader.translating") : t("reader.translateChapter")}
                    </button>
                  </div>
                )}
              </>}
            </div>
          </div>
        )}
        {!panes.orig && !panes.transl && <div className="muted" style={{ padding: 20 }}>{t("reader.bothClosed")}</div>}
      </div>

      {popover && (
        <TermPopover t={t} term={popover.term} rect={popover.rect} onClose={closePopover} onOpenGlossary={onOpenGlossaryTerm} />
      )}
    </div>
  );
}
