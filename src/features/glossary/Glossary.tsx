import { useEffect, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type {
  GlossaryPage,
  GlossaryTermView,
} from "../../shared/contracts/generated";
import { Modal } from "../../shared/ui/Modal";
import { errorText, type T } from "../../app/strings";
export function Glossary({
  projectId,
  t,
  extract,
  canExtract,
}: {
  projectId: string;
  t: T;
  extract: () => Promise<void>;
  canExtract: boolean;
}) {
  const [page, setPage] = useState<GlossaryPage | null>(null),
    [error, setError] = useState<unknown>(null),
    [busy, setBusy] = useState(false),
    [edit, setEdit] = useState<GlossaryTermView | null>(null);
  const [query, setQuery] = useState(""),
    [pinnedOnly, setPinnedOnly] = useState(false);
  const [filter, setFilter] = useState({ query: "", pinnedOnly: false });
  async function load(more = false) {
    setBusy(true);
    setError(null);
    try {
      const next = await projectApi.glossary({
        projectId,
        ...filter,
        cursor: more ? (page?.nextCursor ?? null) : null,
        limit: 100,
      });
      setPage((previous) => ({
        ...next,
        items: more ? [...(previous?.items ?? []), ...next.items] : next.items,
      }));
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    let alive = true;
    setBusy(true);
    setPage(null);
    setError(null);
    void projectApi
      .glossary({ projectId, ...filter, cursor: null, limit: 100 })
      .then((v) => {
        if (alive) setPage(v);
      })
      .catch((e) => {
        if (alive) setError(e);
      })
      .finally(() => {
        if (alive) setBusy(false);
      });
    return () => {
      alive = false;
    };
  }, [projectId, filter]);
  async function saveTerm() {
    if (!edit || !page) return;
    setBusy(true);
    setError(null);
    try {
      await projectApi.putTerm({
        projectId,
        termId: edit.id,
        source: edit.source,
        target: edit.target,
        kind: edit.kind,
        pinned: edit.pinned,
        expectedRevision: page.items.some((v) => v.id === edit.id)
          ? edit.revision
          : null,
        expectedSettingsRevision: page.settingsRevision,
      });
      setEdit(null);
      await load();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="bc-tool bc-glossary">
      <header className="bc-section-heading">
        <h2>{t("glossary")}</h2>
        <div>
          <button
            disabled={busy || !page}
            onClick={() =>
              setEdit({
                id: crypto.randomUUID(),
                source: "",
                target: "",
                kind: "term",
                pinned: true,
                frequency: 0,
                revision: "0",
              })
            }
          >
            {t("newTerm")}
          </button>
          {canExtract && (
            <button
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void extract()
                  .catch(setError)
                  .finally(() => setBusy(false));
              }}
            >
              {t("extractTerms")}
            </button>
          )}
          <button disabled={busy} onClick={() => void load()}>
            {t("refresh")}
          </button>
        </div>
      </header>
      <form
        className="bc-fields"
        onSubmit={(e) => {
          e.preventDefault();
          setFilter({ query, pinnedOnly });
        }}
      >
        <label>
          {t("glossarySearch")}
          <input
            value={query}
            disabled={busy}
            onChange={(e) => setQuery(e.target.value)}
            maxLength={1024}
          />
        </label>
        <label className="bc-check">
          <input
            type="checkbox"
            checked={pinnedOnly}
            disabled={busy}
            onChange={(e) => setPinnedOnly(e.target.checked)}
          />
          {t("pinnedOnly")}
        </label>
        <button disabled={busy}>{t("find")}</button>
      </form>
      {page && (
        <p className="bc-hint" role="status">
          {t("termsFound")}: {page.total}
        </p>
      )}
      <div className="bc-term-grid bc-term-heading">
        <span>{t("termSource")}</span>
        <span>{t("termTarget")}</span>
        <span>{t("termKind")}</span>
        <span>{t("occurrences")}</span>
      </div>
      {page?.items.map((term) => (
        <button
          className="bc-term-grid bc-term"
          key={term.id}
          disabled={busy}
          onClick={() => {
            setError(null);
            setEdit(term);
          }}
        >
          <strong>
            {term.source}
            {term.pinned ? " ◆" : ""}
          </strong>
          <span>{term.target}</span>
          <span>{term.kind}</span>
          <span>{term.frequency}</span>
        </button>
      ))}
      {page && !page.items.length && (
        <p className="bc-hint">
          {t(
            filter.query || filter.pinnedOnly
              ? "noMatchingTerms"
              : "emptyTerms",
          )}
        </p>
      )}
      {page?.nextCursor && (
        <button disabled={busy} onClick={() => void load(true)}>
          {t("more")}
        </button>
      )}
      {error != null && !edit && (
        <p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>
      )}
      {edit && (
        <Modal
          closeLabel={t("close")}
          title={t("glossary")}
          busy={busy}
          onClose={() => setEdit(null)}
        >
          <div className="bc-dialog-body">
            <label>
              {t("termSource")}
              <input
                value={edit.source}
                onChange={(e) => setEdit({ ...edit, source: e.target.value })}
              />
            </label>
            <label>
              {t("termTarget")}
              <input
                autoFocus
                value={edit.target}
                onChange={(e) => setEdit({ ...edit, target: e.target.value })}
              />
            </label>
            <label>
              {t("termKind")}
              <input
                value={edit.kind}
                onChange={(e) => setEdit({ ...edit, kind: e.target.value })}
              />
            </label>
            <label className="bc-check">
              <input
                type="checkbox"
                checked={edit.pinned}
                onChange={(e) => setEdit({ ...edit, pinned: e.target.checked })}
              />
              {t("pinned")}
            </label>
            {error != null && (
              <p role="alert" className="bc-error">
                {errorText(error, t)}
              </p>
            )}
          </div>
          <footer>
            {page?.items.some((v) => v.id === edit.id) && (
              <button
                className="danger"
                disabled={busy}
                onClick={() => {
                  if (!confirm(`${t("remove")}: ${edit.source}?`)) return;
                  setBusy(true);
                  void projectApi
                    .deleteTerm({
                      projectId,
                      termId: edit.id,
                      expectedRevision: edit.revision,
                      expectedSettingsRevision: page.settingsRevision,
                    })
                    .then(async () => {
                      setEdit(null);
                      await load();
                    })
                    .catch(setError)
                    .finally(() => setBusy(false));
                }}
              >
                {t("remove")}
              </button>
            )}
            <button disabled={busy} onClick={() => setEdit(null)}>
              {t("cancel")}
            </button>
            <button
              className="primary"
              disabled={busy || !edit.source.trim() || !edit.target.trim()}
              onClick={() => void saveTerm()}
            >
              {t("save")}
            </button>
          </footer>
        </Modal>
      )}
    </div>
  );
}
