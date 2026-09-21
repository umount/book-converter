import { useEffect, useMemo, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { countMatches, isBadPattern, replaceAllText, replaceNth, selectedText, type FindOpts } from "../lib/find";
import { tokenizeLines } from "../lib/highlight";
import type { FindApi } from "../hooks/useFindReplace";
import type { ChapterRow, ChapterView, Term } from "../types";
import { ResizeHandle } from "./common/ResizeHandle";
import { EditorSurface } from "./reader/EditorSurface";
import { FindReplaceBar } from "./reader/FindReplaceBar";
import { TermPopover } from "./reader/TermPopover";

type PaneState = { orig: boolean; transl: boolean };

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  chapters: ChapterRow[];
  chapterIdx: number | null;
  setChapterIdx: (idx: number) => void;
  chapter: ChapterView | null;
  chapterLoading: boolean;
  panes: PaneState;
  setPanes: Dispatch<SetStateAction<PaneState>>;
  /** Glossary highlighting (a setting, toggled from Settings / the View menu). */
  hl: boolean;
  /** Find/replace state, owned by App so the menu and palette can open it. */
  find: FindApi;
  /** Full glossary (source, target, kind) for highlighting + the term popover. */
  terms: Term[];
  /** Jump to a term's entry in the glossary view. */
  onOpenGlossaryTerm: (source: string) => void;
  /** Literal find/replace across every stored translation; returns changed count. */
  onReplaceInBook: (find: string, replace: string, opts: FindOpts) => Promise<number>;
  /** True while a translation job is running for this project. */
  translating: boolean;
  onTranslateChapter: (idx: number) => void;
  onTranslateChapterTitle: (idx: number) => Promise<void>;
  onSaveTranslation: (idx: number, title: string, body: string) => Promise<void>;
  onSaveChapterPrompt: (idx: number, prompt: string) => Promise<void>;
  onSaveChapterContext: (idx: number, summary: string, prevTail: string) => Promise<void>;
  onRetranslateWithPrompt: (idx: number, prompt: string) => Promise<void>;
  /** Lets App flush the editor draft before the assistant reloads the chapter. */
  flushRef?: MutableRefObject<null | (() => Promise<void>)>;
};

/** Idle time after the last keystroke before the translation is persisted. */
const AUTOSAVE_MS = 900;

/** Horizontal share of the original pane when both are open. */
const LS_SPLIT = "bc.reader.split";
const MIN_SHARE = 0.15;
const MAX_SHARE = 0.85;

/**
 * Pane visibility toggle, IDE style: each pane is closed by its own × and
 * brought back from here, instead of switching between layout presets.
 */
function PaneToggle({
  side, on, title, onToggle,
}: { side: "left" | "right"; on: boolean; title: string; onToggle: () => void }) {
  return (
    <button className={`act-btn pane-toggle ${on ? "active" : ""}`} title={title} onClick={onToggle}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round">
        {on && <rect x={side === "left" ? 4 : 12} y="5.5" width="8" height="13" fill="currentColor" stroke="none" />}
        <rect x="4" y="5.5" width="16" height="13" rx="1.5" />
        <path d="M12 5.5v13" />
      </svg>
    </button>
  );
}

export function Reader({
  t, chapters, chapterIdx, setChapterIdx, chapter, chapterLoading,
  panes, setPanes, hl, find, terms, onOpenGlossaryTerm, onReplaceInBook,
  translating, onTranslateChapter, onTranslateChapterTitle, onSaveTranslation, onSaveChapterPrompt,
  onSaveChapterContext, onRetranslateWithPrompt, flushRef,
}: Props) {
  // The translation pane is always editable (no edit mode): the text lives in a
  // draft that is autosaved, and flushed when leaving the chapter.
  const [titleDraft, setTitleDraft] = useState("");
  const [bodyDraft, setBodyDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [promptDraft, setPromptDraft] = useState("");
  const [promptBusy, setPromptBusy] = useState(false);
  const [titleBusy, setTitleBusy] = useState(false);
  const [contextOpen, setContextOpen] = useState(false);
  const [summaryDraft, setSummaryDraft] = useState("");
  const [tailDraft, setTailDraft] = useState("");
  const [contextBusy, setContextBusy] = useState(false);

  // Glossary occurrence highlight: the active term key + the popover it opened.
  const [activeTerm, setActiveTerm] = useState<string | null>(null);
  const [popover, setPopover] = useState<{ term: Term; rect: DOMRect } | null>(null);

  // Sync drafts on chapter change. Context panel always closes (opt-in only).
  useEffect(() => {
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

  // --- Inline editing of the translation ---------------------------------
  //
  // Adopt the stored translation into the drafts, except when it is the echo of
  // our own save coming back: that would clobber keystrokes typed while the save
  // was in flight. Anything else (a regenerate, a book-wide replace) wins.
  const lastSentRef = useRef<{ title: string; body: string } | null>(null);
  // Which chapter the drafts currently hold. A draft is only ever saved back to
  // the chapter it was loaded from: `chapterIdx` changes as soon as the user
  // navigates, while the drafts catch up a render later, and pairing the new
  // index with the old draft is how a translation gets overwritten.
  const draftIdxRef = useRef<number | null>(null);
  useEffect(() => {
    const body = chapter?.translated ?? "";
    const title = chapter?.translated_title ?? "";
    const sent = lastSentRef.current;
    if (sent && sent.body === body.trim() && sent.title === title.trim()) {
      draftIdxRef.current = chapter?.idx ?? null;
      return;
    }
    lastSentRef.current = null;
    setBodyDraft(body);
    setTitleDraft(title);
    draftIdxRef.current = chapter?.idx ?? null;
  }, [chapterIdx, chapter?.idx, chapter?.translated, chapter?.translated_title]);

  // A chapter can hold a translation while not being `done`: a failed run, a
  // reset queued for re-translation, or one being translated right now. Hiding
  // the stored text in those states loses work the search can still find, so the
  // text is shown whenever it exists, and the state is stated in the header.
  const hasTranslation = !!chapter?.translated?.trim();
  const chapterBusy = chapter?.status === "in_progress";
  // The backend refuses edits to a chapter mid-translation (`chapter_busy`).
  const canEdit = hasTranslation && !chapterBusy;
  // Words the run could not get out of the translation (flagged in the tree too).
  const langIssues = chapters.find((c) => c.idx === chapterIdx)?.lang_issues ?? null;

  // Compared trimmed, because that is what the backend stores: otherwise a
  // trailing newline would look dirty forever and autosave in a loop.
  // Only the chapter the drafts were loaded from can be dirty. Without this, the
  // render between a freshly loaded translation and the drafts adopting it looks
  // like "the user cleared the whole chapter".
  const draftsMatchChapter = chapterIdx != null && draftIdxRef.current === chapterIdx;
  const dirty =
    canEdit &&
    draftsMatchChapter &&
    (bodyDraft.trim() !== (chapter?.translated ?? "").trim() ||
      titleDraft.trim() !== (chapter?.translated_title ?? "").trim());

  // Live handles for the flush that runs on chapter change / unmount, where the
  // rendered values are already gone.
  const pendingRef = useRef<{ idx: number; title: string; body: string } | null>(null);
  const saveRef = useRef(onSaveTranslation);
  saveRef.current = onSaveTranslation;
  useEffect(() => {
    pendingRef.current = dirty && chapterIdx != null ? { idx: chapterIdx, title: titleDraft, body: bodyDraft } : null;
    if (flushRef) flushRef.current = flushEdits;
  });

  async function flushEdits() {
    const p = pendingRef.current;
    if (!p) return;
    pendingRef.current = null;
    // Emptying a chapter is never something autosave should do on its own.
    if (!p.body.trim()) return;
    lastSentRef.current = { title: p.title.trim(), body: p.body.trim() };
    setSaving(true);
    try {
      await saveRef.current(p.idx, p.title, p.body);
    } finally {
      setSaving(false);
    }
  }

  useEffect(() => {
    if (!dirty) return;
    const id = setTimeout(() => void flushEdits(), AUTOSAVE_MS);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dirty, bodyDraft, titleDraft]);

  // Leaving the chapter (or the reader) must not drop unsaved keystrokes.
  useEffect(
    () => () => {
      const p = pendingRef.current;
      if (!p) return;
      pendingRef.current = null;
      if (!p.body.trim()) return; // see flushEdits
      void saveRef.current(p.idx, p.title, p.body);
    },
    [chapterIdx],
  );

  const termMap = useMemo(() => new Map(terms.map((tm) => [tm.source, tm])), [terms]);
  const srcMatches = useMemo(() => terms.map((tm) => ({ match: tm.source, key: tm.source })), [terms]);
  const tgtMatches = useMemo(() => terms.map((tm) => ({ match: tm.target, key: tm.source })), [terms]);

  const [replaceBusy, setReplaceBusy] = useState(false);
  // Search and highlighting run on the draft, so they follow what is on screen.
  const translation = bodyDraft;
  const findOpts = useMemo<FindOpts>(
    () => ({ matchCase: find.matchCase, wholeWord: find.wholeWord, regex: find.regex }),
    [find.matchCase, find.wholeWord, find.regex],
  );
  const badPattern = isBadPattern(find.query, findOpts);
  const searchSpec = useMemo(
    () => (find.open && find.query ? { query: find.query, opts: findOpts } : null),
    [find.open, find.query, findOpts],
  );
  const matchCount = useMemo(
    () => (searchSpec ? countMatches(translation, searchSpec.query, findOpts) : 0),
    [searchSpec, translation, findOpts],
  );

  const sourceLines = useMemo(
    () => tokenizeLines(chapter?.source ?? "", hl ? srcMatches : []),
    [chapter?.source, srcMatches, hl],
  );
  const translLines = useMemo(
    () => tokenizeLines(translation, hl ? tgtMatches : [], searchSpec),
    [translation, tgtMatches, hl, searchSpec],
  );

  // Keep the active match index valid across query / chapter changes.
  useEffect(() => {
    if (find.current >= matchCount) find.setCurrent(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [matchCount]);
  useEffect(() => {
    find.setCurrent(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [find.query, find.matchCase, find.wholeWord, find.regex, chapterIdx]);

  // Seed the query from what is selected in the editor, the way ⌘F does in an IDE.
  const setSeeder = find.setSeeder;
  useEffect(() => {
    setSeeder(selectedText);
  }, [setSeeder]);

  function findNext() { if (matchCount) find.setCurrent((find.current + 1) % matchCount); }
  function findPrev() { if (matchCount) find.setCurrent((find.current - 1 + matchCount) % matchCount); }

  async function replaceOne() {
    if (chapterIdx == null || !find.query || matchCount === 0) return;
    const next = replaceNth(translation, find.query, find.replacement, findOpts, find.current);
    if (next !== translation) setBodyDraft(next); // autosaved like any other edit
  }
  async function replaceAll() {
    if (!find.query) return;
    if (find.scope === "book") {
      if (!confirm(t("find.replaceBookConfirm", { find: find.query, replace: find.replacement }))) return;
      setReplaceBusy(true);
      try { await onReplaceInBook(find.query, find.replacement, findOpts); }
      finally { setReplaceBusy(false); }
      return;
    }
    if (chapterIdx == null) return;
    const { text: next, count } = replaceAllText(translation, find.query, find.replacement, findOpts);
    if (count > 0) setBodyDraft(next);
  }

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
    if (chapterIdx == null || translating) return;
    // A fresh translation overwrites the text anyway: drop pending edits so a
    // late autosave cannot land on top of the new translation.
    pendingRef.current = null;
    setPromptBusy(true);
    try {
      await onRetranslateWithPrompt(chapterIdx, promptDraft);
    } finally {
      setPromptBusy(false);
    }
  }

  async function translateTitleOnly() {
    if (chapterIdx == null || chapterBusy || titleBusy) return;
    await flushEdits();
    pendingRef.current = null;
    setTitleBusy(true);
    try {
      await onTranslateChapterTitle(chapterIdx);
    } finally {
      setTitleBusy(false);
    }
  }

  const pos = chapters.findIndex((c) => c.idx === chapterIdx);
  const gotoRel = (d: number) => {
    const j = pos + d;
    if (pos >= 0 && j >= 0 && j < chapters.length) setChapterIdx(chapters[j].idx);
  };

  // Split position: the original pane's share of the row, dragged from the
  // divider and remembered across sessions.
  const both = panes.orig && panes.transl;
  const panesRef = useRef<HTMLDivElement>(null);
  const [share, setShare] = useState(() => {
    const v = Number(localStorage.getItem(LS_SPLIT));
    return v >= MIN_SHARE && v <= MAX_SHARE ? v : 0.5;
  });
  const shareStart = useRef(share);
  function dragSplit(dx: number) {
    const w = panesRef.current?.clientWidth ?? 0;
    if (!w) return;
    setShare(Math.min(MAX_SHARE, Math.max(MIN_SHARE, shareStart.current + dx / w)));
  }
  // Closing a pane must not leave the other one pinned to a fraction of the row.
  const origStyle = both ? { flex: `0 0 calc(${(share * 100).toFixed(2)}% - 5px)` } : undefined;


  const canTranslate = !!chapter && chapterIdx != null && !translating;
  const canRegenerate = canTranslate && !promptBusy;
  const canTranslateTitle = !!chapter?.source_title?.trim() && !chapterBusy && !titleBusy;
  const hasPrompt = !!(promptDraft.trim() || chapter?.user_prompt);
  const hasContext = !!(summaryDraft.trim() || tailDraft.trim() || chapter?.rolling_summary || chapter?.prev_tail);

  return (
    <div className="reader">
      <div className="reader-toolbar">
        <button className="ghost" disabled={pos <= 0} onClick={() => gotoRel(-1)} title={t("reader.prev")}>‹</button>
        <button className="ghost" disabled={pos < 0 || pos >= chapters.length - 1} onClick={() => gotoRel(1)} title={t("reader.next")}>›</button>
        <div className="menu-spacer" />
        <PaneToggle
          side="left" on={panes.orig} title={t("reader.showOriginal")}
          onToggle={() => setPanes((p) => ({ ...p, orig: !p.orig }))}
        />
        <PaneToggle
          side="right" on={panes.transl} title={t("reader.showTranslation")}
          onToggle={() => setPanes((p) => ({ ...p, transl: !p.transl }))}
        />
        <button className={`chip ${promptOpen || hasPrompt ? "on" : ""}`} title={t("reader.promptTip")} disabled={!chapter}
          onClick={() => { setPromptOpen((o) => !o); if (!promptOpen) setContextOpen(false); }}>
          {t("reader.prompt")}{hasPrompt ? " ·" : ""}
        </button>
        <button className={`chip ${contextOpen ? "on" : ""}`} title={t("reader.contextTip")} disabled={!chapter}
          onClick={() => { setContextOpen((o) => !o); if (!contextOpen) setPromptOpen(false); }}>
          {t("reader.context")}{hasContext && !contextOpen ? " ·" : ""}
        </button>
      </div>

      {find.open && chapter && (
        <FindReplaceBar
          t={t} find={find} count={matchCount} badPattern={badPattern} busy={replaceBusy}
          onPrev={findPrev} onNext={findNext}
          onReplaceOne={() => void replaceOne()} onReplaceAll={() => void replaceAll()}
        />
      )}

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

      <div className="panes" ref={panesRef}>
        {panes.orig && (
          <div className="pane" style={origStyle}>
            <div className="pane-head">
              <span className="pane-title" title={`${t("reader.original")}: ${chapter?.source_title ?? ""}`}>
                {chapter?.source_title}
              </span>
              <button className="icon" onClick={() => setPanes((p) => ({ ...p, orig: false }))}>×</button>
            </div>
            <div className="pane-body">
              {chapterLoading ? <div className="loading"><span className="spinner" /> {t("reader.loading")}</div> : (
                <EditorSurface lines={sourceLines} activeKey={activeTerm} onTermClick={onTermClick} />
              )}
            </div>
          </div>
        )}
        {both && (
          <ResizeHandle
            axis="x"
            onStart={() => { shareStart.current = share; }}
            onDrag={dragSplit}
            onEnd={() => localStorage.setItem(LS_SPLIT, String(share))}
          />
        )}
        {panes.transl && (
          <div className="pane">
            <div className="pane-head">
              {canEdit ? (
                <input
                  className="pane-title pane-title-input" value={titleDraft}
                  onChange={(e) => setTitleDraft(e.target.value)}
                  placeholder={t("reader.editTitlePlaceholder")}
                  title={`${t("reader.translation")}: ${titleDraft}`}
                />
              ) : (
                <span className="pane-title" title={t("reader.translation")}>
                  {chapter?.translated_title || t("reader.notTranslated")}
                </span>
              )}
              {chapter && chapter.status !== "done" && (
                <span className={`ref-badge state-${chapter.status}`} title={t(`chapterState.${chapter.status}Tip`)}>
                  {t(`chapterState.${chapter.status}`)}
                </span>
              )}
              {langIssues && (
                <span className="ref-badge state-issues" title={t("reader.langIssuesTip", { words: langIssues })}>
                  {t("reader.langIssues")}
                </span>
              )}
              {chapter?.origin === "reference" && <span className="ref-badge" title={t("reader.fromReferenceTip")}>{t("reader.fromReference")}</span>}
              {chapter?.origin === "manual" && <span className="ref-badge" title={t("reader.manualTip")}>{t("reader.manual")}</span>}
              {chapter && (
                <button className="ghost" disabled={!canTranslateTitle} onClick={() => void translateTitleOnly()} title={t("reader.translateTitleTip")}>
                  {titleBusy ? t("reader.translating") : t("reader.translateTitle")}
                </button>
              )}
              {canEdit && (
                <>
                  <span className={`save-state ${saving ? "busy" : dirty ? "dirty" : ""}`} title={t("reader.autosaveTip")}>
                    {saving ? t("reader.saving") : dirty ? t("reader.unsaved") : t("reader.saved")}
                  </span>
                  <button className="ghost" disabled={!canRegenerate} onClick={() => void regenerate()} title={t("reader.regenerateTip")}>
                    {promptBusy || translating ? t("reader.translating") : t("reader.regenerate")}
                  </button>
                </>
              )}
              <button className="icon" onClick={() => setPanes((p) => ({ ...p, transl: false }))}>×</button>
            </div>
            <div className="pane-body">
              {chapterLoading ? <div className="loading"><span className="spinner" /> {t("reader.loading")}</div> : hasTranslation ? (
                <EditorSurface
                  lines={translLines} activeKey={activeTerm} onTermClick={onTermClick}
                  currentSearch={searchSpec ? find.current : undefined}
                  editable={canEdit} value={bodyDraft} onChange={setBodyDraft}
                />
              ) : (
                <div className="ch-empty">
                  <button disabled={!canTranslate} onClick={() => chapterIdx != null && onTranslateChapter(chapterIdx)}>
                    {translating ? t("reader.translating") : t("reader.translateChapter")}
                  </button>
                </div>
              )}
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
