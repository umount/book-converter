import { useConfirm } from "../../shared/ui/useConfirm";
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
  onJob,
}: {
  onJob: (
    job: import("../../shared/contracts/generated").JobRef,
  ) => Promise<void>;
  projectId: string;
  t: T;
  extract: (maxChapters: number, force: boolean) => Promise<void>;
  canExtract: boolean;
}) {
  const confirmation = useConfirm(t);
  const [page, setPage] = useState<GlossaryPage | null>(null),
    [error, setError] = useState<unknown>(null),
    [busy, setBusy] = useState(false),
    [edit, setEdit] = useState<GlossaryTermView | null>(null);
  const [showExtraction, setShowExtraction] = useState(false);
  const [extractionCount, setExtractionCount] = useState("10");
  const [repeatExtraction, setRepeatExtraction] = useState(false);
  const validExtractionCount =
    /^\d+$/.test(extractionCount) &&
    Number(extractionCount) > 0 &&
    Number(extractionCount) <= 4294967295;
  const [query, setQuery] = useState(""),
    [pinnedOnly, setPinnedOnly] = useState(false);
  const [offer, setOffer] = useState<{
    termId: string;
    revision: string;
    oldTarget: string;
    target: string;
  } | null>(null);
  const [showOffer, setShowOffer] = useState(false);
  const [estimate, setEstimate] = useState<
    import("../../shared/contracts/generated").BookRetargetPreview | null
  >(null);
  const [retargetCount, setRetargetCount] = useState("10");
  const validCount =
    /^\d+$/.test(retargetCount) &&
    Number(retargetCount) > 0 &&
    Number(retargetCount) <= 4294967295;
  useEffect(() => {
    let alive = true;
    setEstimate(null);
    if (!offer || !showOffer || !validCount) return;
    const timer = setTimeout(
      () =>
        void projectApi
          .retargetPreview({
            projectId,
            termId: offer.termId,
            expectedRevision: offer.revision,
            oldTarget: offer.oldTarget,
            maxChapters: Number(retargetCount),
          })
          .then((value) => {
            if (alive) setEstimate(value);
          })
          .catch((e) => {
            if (alive) setError(e);
          }),
      200,
    );
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [offer, showOffer, retargetCount, validCount, projectId]);
  const [filter, setFilter] = useState({ query: "", pinnedOnly: false });
  useEffect(() => {
    const timer = setTimeout(
      () =>
        setFilter((previous) =>
          previous.query === query && previous.pinnedOnly === pinnedOnly
            ? previous
            : { query, pinnedOnly },
        ),
      250,
    );
    return () => clearTimeout(timer);
  }, [query, pinnedOnly]);
  const kindLabel = (kind: string) => {
    switch (kind) {
      case "character":
      case "person":
        return t("kindPerson");
      case "place":
      case "location":
        return t("kindLocation");
      case "organization":
        return t("kindOrganization");
      case "term":
        return t("kindTerm");
      default:
        return kind;
    }
  };
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
      const saved = page.items.find((v) => v.id === edit.id);
      const unchanged =
        saved &&
        saved.source === edit.source &&
        saved.target === edit.target &&
        saved.kind === edit.kind &&
        saved.pinned === edit.pinned;
      const revision = unchanged
        ? edit.revision
        : await projectApi.putTerm({
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
      if (canExtract && saved && saved.target !== edit.target) {
        setOffer({
          termId: edit.id,
          revision,
          oldTarget: saved.target,
          target: edit.target,
        });
        setShowOffer(true);
      }
      await load();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="bc-tool bc-glossary">
      {confirmation.dialog}
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
                setError(null);
                setShowExtraction(true);
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
      {offer && (
        <button
          disabled={busy}
          onClick={() => {
            setError(null);
            setShowOffer(true);
          }}
        >
          {t("retarget")}: {offer.oldTarget} → {offer.target}
        </button>
      )}
      <form
        className="bc-glossary-search"
        role="search"
        onSubmit={(e) => {
          e.preventDefault();
          setFilter({ query, pinnedOnly });
        }}
      >
        <label>
          <span className="bc-sr-only">{t("glossarySearch")}</span>
          <input
            type="search"
            placeholder={t("glossarySearchShort")}
            title={t("glossarySearch")}
            value={query}
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
          <span className="bc-kind-badge" data-kind={term.kind}>
            {kindLabel(term.kind)}
          </span>
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
                onClick={async () => {
                  if (
                    !(await confirmation.confirm({
                      title: t("remove"),
                      message: `${t("remove")}: ${edit.source}?`,
                      action: t("remove"),
                      danger: true,
                    }))
                  )
                    return;
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
      {showExtraction && (
        <Modal
          title={t("extractTerms")}
          closeLabel={t("close")}
          busy={busy}
          onClose={() => setShowExtraction(false)}
        >
          <div className="bc-dialog-body">
            <p className="bc-hint">{t("extractionBatchHint")}</p>
            <label>
              {t("batchCount")}
              <input
                type="number"
                min="1"
                max="4294967295"
                value={extractionCount}
                disabled={busy}
                onChange={(e) => setExtractionCount(e.target.value)}
              />
            </label>
            <label>
              <input
                type="checkbox"
                checked={repeatExtraction}
                disabled={busy}
                onChange={(e) => setRepeatExtraction(e.target.checked)}
              />
              {t("repeatExtraction")}
            </label>
            {error != null && (
              <p className="bc-error" role="alert">
                {errorText(error, t)}
              </p>
            )}
          </div>
          <footer>
            <button disabled={busy} onClick={() => setShowExtraction(false)}>
              {t("cancel")}
            </button>
            <button
              className="primary"
              disabled={busy || !validExtractionCount}
              onClick={() => {
                setBusy(true);
                setError(null);
                void extract(Number(extractionCount), repeatExtraction)
                  .then(() => setShowExtraction(false))
                  .catch(setError)
                  .finally(() => setBusy(false));
              }}
            >
              {t("extractTerms")}
            </button>
          </footer>
        </Modal>
      )}
      {offer && showOffer && (
        <Modal
          title={t("retarget")}
          closeLabel={t("close")}
          busy={busy}
          onClose={() => setShowOffer(false)}
        >
          <div className="bc-dialog-body">
            <p>
              {offer.oldTarget} → {offer.target}
            </p>
            <p className="bc-hint">{t("retargetHint")}</p>
            <label>
              {t("batchCount")}
              <input
                type="number"
                min="1"
                max="4294967295"
                value={retargetCount}
                disabled={busy}
                onChange={(e) => setRetargetCount(e.target.value)}
              />
            </label>
            <p role="status">
              {estimate
                ? `${t("chapters")}: ${estimate.chapters} · ${t("retargetFragments")}: ${estimate.fragments}`
                : validCount
                  ? t("loading")
                  : t("batchCount")}
            </p>
            {error != null && (
              <p className="bc-error" role="alert">
                {errorText(error, t)}
              </p>
            )}
          </div>
          <footer>
            <button disabled={busy} onClick={() => setShowOffer(false)}>
              {t("cancel")}
            </button>
            <button
              className="primary"
              disabled={busy || !validCount || !estimate?.chapters}
              onClick={() =>
                void (async () => {
                  setBusy(true);
                  setError(null);
                  try {
                    const job = await projectApi.retarget({
                      projectId,
                      termId: offer.termId,
                      expectedRevision: offer.revision,
                      oldTarget: offer.oldTarget,
                      maxChapters: Number(retargetCount),
                    });
                    await onJob(job);
                    setShowOffer(false);
                    setEstimate(null);
                  } catch (e) {
                    setError(e);
                  } finally {
                    setBusy(false);
                  }
                })()
              }
            >
              {t("retarget")}
            </button>
          </footer>
        </Modal>
      )}
    </div>
  );
}
