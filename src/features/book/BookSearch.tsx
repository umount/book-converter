import { useEffect, useRef, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type {
  BookSearchMatch,
  BookSearchSide,
  BookReplacePreview,
  ChapterSummary,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";
export function BookSearch({
  active,
  close,
  projectId,
  t,
  open,
  mode,
  setMode,
  chapterId,
  chapters,
  beforeWork,
  refresh,
  registerFlush,
}: {
  active: boolean;
  close: () => void;
  projectId: string;
  t: T;
  open: (chapterId: string, blockId: string) => void;
  mode: "find" | "replace";
  setMode: (mode: "find" | "replace") => void;
  chapterId: string | null;
  chapters: ChapterSummary[];
  beforeWork: () => Promise<void>;
  refresh: () => Promise<void>;
  registerFlush: (flush: (() => Promise<void>) | null) => void;
}) {
  const [query, setQuery] = useState("");
  const [side, setSide] = useState<BookSearchSide>("translation");
  const [replacement, setReplacement] = useState("");
  const [scope, setScope] = useState("all");
  const [preview, setPreview] = useState<BookReplacePreview | null>(null);
  const [matchCase, setMatchCase] = useState(false);
  const [matches, setMatches] = useState<BookSearchMatch[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false),
    [searched, setSearched] = useState(false),
    [error, setError] = useState<unknown>(null),
    [notice, setNotice] = useState("");
  const alive = useRef(true),
    working = useRef(false),
    generation = useRef(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (active) input.current?.focus();
  }, [active, mode]);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      ++generation.current;
    };
  }, []);
  useEffect(() => {
    registerFlush(async () => {
      if (working.current) throw new Error(t("processing"));
    });
    return () => registerFlush(null);
  }, [registerFlush, t]);
  function reset() {
    ++generation.current;
    setMatches([]);
    setCursor(null);
    setSearched(false);
    setPreview(null);
    setNotice("");
  }
  useEffect(() => {
    reset();
  }, [mode, chapterId]);
  async function run(more = false, apply = false) {
    if (working.current || !query.trim()) return;
    working.current = true;
    setBusy(true);
    setError(null);
    setNotice("");
    const request = ++generation.current;
    try {
      if (mode === "replace") {
        await beforeWork();
        if (apply && preview) {
          await projectApi.applyReplace({
            projectId,
            previewId: preview.previewId,
          });
          await refresh();
          if (alive.current) {
            reset();
            setNotice(t("saved"));
          }
        } else {
          const value = await projectApi.previewReplace({
            projectId,
            selection:
              scope === "chapter" && chapterId
                ? { kind: "explicit_ids", ids: [chapterId] }
                : { kind: "all" },
            search: query,
            replacement,
            caseSensitive: matchCase,
          });
          if (alive.current && request === generation.current)
            setPreview(value);
        }
      } else {
        const page = await projectApi.searchBook({
          projectId,
          query,
          side,
          caseSensitive: matchCase,
          cursor: more ? cursor : null,
          limit: 50,
        });
        if (alive.current && request === generation.current) {
          setMatches((previous) =>
            more ? [...previous, ...page.matches] : page.matches,
          );
          setCursor(page.nextCursor);
          setSearched(true);
        }
      }
    } catch (e) {
      if (alive.current) {
        setError(e);
        if (apply) setPreview(null);
      }
    } finally {
      working.current = false;
      if (alive.current) setBusy(false);
    }
  }
  return (
    <section
      className="bc-book-search"
      hidden={!active}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) close();
      }}
    >
      <header>
        <div
          className="bc-search-modes"
          role="group"
          aria-label={t("bookSearch")}
        >
          <button
            aria-pressed={mode === "find"}
            disabled={busy}
            onClick={() => setMode("find")}
          >
            {t("find")}
          </button>
          <button
            aria-pressed={mode === "replace"}
            disabled={busy}
            onClick={() => setMode("replace")}
          >
            {t("replaceMode")}
          </button>
        </div>
        <button
          className="bc-icon-button"
          aria-label={t("close")}
          disabled={busy}
          onClick={close}
        >
          ×
        </button>
      </header>
      <form
        className="bc-search-form"
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <label>
          <span className="bc-sr-only">{t("searchText")}</span>
          <input
            ref={input}
            placeholder={t("searchText")}
            disabled={busy}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              reset();
            }}
          />
        </label>
        {mode === "replace" ? (
          <>
            <label>
              <span className="bc-sr-only">{t("replacement")}</span>
              <input
                placeholder={t("replacement")}
                disabled={busy}
                value={replacement}
                onChange={(e) => {
                  setReplacement(e.target.value);
                  reset();
                }}
              />
            </label>
            <small className="bc-hint">{t("replaceTranslationOnly")}</small>
            <label>
              {t("selection")}
              <select
                disabled={busy}
                value={scope}
                onChange={(e) => {
                  setScope(e.target.value);
                  reset();
                }}
              >
                <option value="all">{t("allChapters")}</option>
                <option value="chapter" disabled={!chapterId}>
                  {t("selectedChapter")}
                </option>
              </select>
            </label>
          </>
        ) : (
          <label>
            {t("selection")}
            <select
              disabled={busy}
              value={side}
              onChange={(e) => {
                setSide(e.target.value as BookSearchSide);
                reset();
              }}
            >
              <option value="translation">{t("translation")}</option>
              <option value="source">{t("original")}</option>
            </select>
          </label>
        )}
        <label className="bc-check">
          <input
            type="checkbox"
            disabled={busy}
            checked={matchCase}
            onChange={(e) => {
              setMatchCase(e.target.checked);
              reset();
            }}
          />
          {t("caseSensitive")}
        </label>
        <button className="primary" disabled={busy || !query.trim()}>
          {busy
            ? t("loading")
            : t(mode === "replace" ? "previewChanges" : "find")}
        </button>
      </form>
      {error != null && (
        <p className="bc-error" role="alert">
          {errorText(error, t)}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
      {mode === "find" ? (
        <>
          {searched && !matches.length && (
            <p className="bc-hint">{t("noMatches")}</p>
          )}
          <div className="bc-search-results">
            {matches.map((m) => (
              <button
                key={m.blockId}
                disabled={busy}
                onClick={() => open(m.chapterId, m.blockId)}
              >
                <strong>{m.title}</strong>
                <span>{m.snippet}</span>
              </button>
            ))}
          </div>
          {cursor && (
            <button disabled={busy} onClick={() => void run(true)}>
              {t("moreResults")}
            </button>
          )}
        </>
      ) : (
        preview && (
          <>
            <p>
              {t("changes")}: {preview.changes.length}
            </p>
            <button
              className="primary"
              disabled={busy || !preview.changes.length}
              onClick={() => void run(false, true)}
            >
              {t("apply")}
            </button>
            <div className="bc-change-list">
              {preview.changes.map((change) => (
                <article key={change.blockId}>
                  <strong>
                    {chapters.find((c) => c.id === change.chapterId)?.title}
                  </strong>
                  <del>{change.before}</del>
                  <ins>{change.after}</ins>
                </article>
              ))}
            </div>
          </>
        )
      )}
    </section>
  );
}
