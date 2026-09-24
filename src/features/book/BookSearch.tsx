import { useEffect, useRef, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type {
  BookSearchMatch,
  BookSearchSide,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";
export function BookSearch({
  active,
  close,
  projectId,
  t,
  open,
}: {
  active: boolean;
  close: () => void;
  projectId: string;
  t: T;
  open: (chapterId: string, blockId: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [side, setSide] = useState<BookSearchSide>("translation");
  const [matchCase, setMatchCase] = useState(false);
  const [matches, setMatches] = useState<BookSearchMatch[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false),
    [searched, setSearched] = useState(false),
    [error, setError] = useState<unknown>(null);
  const alive = useRef(true);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (active) input.current?.focus();
  }, [active]);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  function reset() {
    setMatches([]);
    setCursor(null);
    setSearched(false);
  }
  async function search(more = false) {
    if (busy || !query.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const page = await projectApi.searchBook({
        projectId,
        query,
        side,
        caseSensitive: matchCase,
        cursor: more ? cursor : null,
        limit: 50,
      });
      if (alive.current) {
        setMatches((previous) =>
          more ? [...previous, ...page.matches] : page.matches,
        );
        setCursor(page.nextCursor);
        setSearched(true);
      }
    } catch (e) {
      if (alive.current) setError(e);
    } finally {
      if (alive.current) setBusy(false);
    }
  }
  return (
    <section
      className="bc-book-search"
      hidden={!active}
      onKeyDown={(e) => {
        if (e.key === "Escape") close();
      }}
    >
      <header>
        <strong>{t("bookSearch")}</strong>
        <button
          className="bc-icon-button"
          aria-label={t("close")}
          title={t("close")}
          onClick={close}
        >
          ×
        </button>
      </header>
      <form
        className="bc-search-form"
        onSubmit={(e) => {
          e.preventDefault();
          void search();
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
          {busy ? t("loading") : t("find")}
        </button>
      </form>
      {error != null && (
        <p className="bc-error" role="alert">
          {errorText(error, t)}
        </p>
      )}
      {searched && !matches.length && (
        <p className="bc-hint">{t("noMatches")}</p>
      )}
      <div className="bc-search-results">
        {matches.map((m) => (
          <button key={m.blockId} onClick={() => open(m.chapterId, m.blockId)}>
            <strong>{m.title}</strong>
            <span>{m.snippet}</span>
          </button>
        ))}
      </div>
      {cursor && (
        <button disabled={busy} onClick={() => void search(true)}>
          {t("moreResults")}
        </button>
      )}
    </section>
  );
}
