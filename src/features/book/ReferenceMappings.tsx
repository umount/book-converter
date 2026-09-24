import { useMemo, useState } from "react";
import type {
  BookReferenceView,
  ChapterSummary,
} from "../../shared/contracts/generated";
import type { T } from "../../app/strings";

const PAGE_SIZE = 50;
export function ReferenceMappings({
  chapters,
  reference,
  disabled,
  onChange,
  t,
}: {
  chapters: ChapterSummary[];
  reference: BookReferenceView;
  disabled: boolean;
  onChange: (value: BookReferenceView) => void;
  t: T;
}) {
  const [chapterQuery, setChapterQuery] = useState("");
  const [referenceQuery, setReferenceQuery] = useState("");
  const [chapterPage, setChapterPage] = useState(0);
  const [referencePage, setReferencePage] = useState(0);
  const [selected, setSelected] = useState<string | null>(null);
  const mappings = useMemo(
    () => new Map(reference.mappings.map((m) => [m.chapterId, m.referenceId])),
    [reference.mappings],
  );
  const byId = useMemo(
    () => new Map(reference.chapters.map((c) => [c.id, c])),
    [reference.chapters],
  );
  const sources = useMemo(
    () =>
      chapters.filter((c) =>
        `${c.position + 1} ${c.title}`
          .toLocaleLowerCase()
          .includes(chapterQuery.toLocaleLowerCase()),
      ),
    [chapters, chapterQuery],
  );
  const targets = useMemo(
    () =>
      reference.chapters.filter((c) =>
        `${c.position + 1} ${c.title}`
          .toLocaleLowerCase()
          .includes(referenceQuery.toLocaleLowerCase()),
      ),
    [reference.chapters, referenceQuery],
  );
  const active = chapters.find((c) => c.id === selected);
  const mapped = active ? byId.get(mappings.get(active.id) ?? "") : undefined;
  function assign(referenceId: string | null) {
    if (!active) return;
    onChange({
      ...reference,
      mappings: [
        ...reference.mappings.filter((m) => m.chapterId !== active.id),
        ...(referenceId ? [{ chapterId: active.id, referenceId }] : []),
      ],
    });
  }
  function pager(page: number, count: number, change: (page: number) => void) {
    return (
      <div className="bc-reference-pager">
        <button
          disabled={disabled || page === 0}
          onClick={() => change(page - 1)}
        >
          {t("previousPage")}
        </button>
        <span>
          {Math.min(page * PAGE_SIZE + 1, count)}–
          {Math.min((page + 1) * PAGE_SIZE, count)} / {count}
        </span>
        <button
          disabled={disabled || (page + 1) * PAGE_SIZE >= count}
          onClick={() => change(page + 1)}
        >
          {t("nextPage")}
        </button>
      </div>
    );
  }
  return (
    <div className="bc-reference-mapping">
      <p className="bc-hint">{t("mappingHint")}</p>
      <div className="bc-reference-columns">
        <section aria-label={t("original")}>
          <label>
            {t("original")}
            <input
              value={chapterQuery}
              disabled={disabled}
              onChange={(e) => {
                setChapterQuery(e.target.value);
                setChapterPage(0);
              }}
            />
          </label>
          {pager(chapterPage, sources.length, setChapterPage)}
          <div className="bc-reference-list">
            {sources
              .slice(chapterPage * PAGE_SIZE, (chapterPage + 1) * PAGE_SIZE)
              .map((c) => (
                <button
                  key={c.id}
                  disabled={disabled}
                  aria-pressed={selected === c.id}
                  onClick={() => setSelected(c.id)}
                >
                  <strong>
                    {c.position + 1}. {c.title}
                  </strong>
                  <small>
                    {byId.get(mappings.get(c.id) ?? "")?.title ?? t("unmapped")}
                  </small>
                </button>
              ))}
          </div>
        </section>
        <section aria-label={t("reference")}>
          <label>
            {t("reference")}
            <input
              value={referenceQuery}
              disabled={disabled}
              onChange={(e) => {
                setReferenceQuery(e.target.value);
                setReferencePage(0);
              }}
            />
          </label>
          {pager(referencePage, targets.length, setReferencePage)}
          <div className="bc-reference-list">
            {targets
              .slice(referencePage * PAGE_SIZE, (referencePage + 1) * PAGE_SIZE)
              .map((c) => (
                <button
                  key={c.id}
                  disabled={disabled || !active}
                  aria-pressed={mapped?.id === c.id}
                  onClick={() => assign(c.id)}
                >
                  {c.position + 1}. {c.title}
                </button>
              ))}
          </div>
        </section>
      </div>
      {active ? (
        <div className="bc-reference-selection">
          <strong>{active.title}</strong>
          <span> → {mapped?.title ?? t("unmapped")}</span>
          <button disabled={disabled || !mapped} onClick={() => assign(null)}>
            {t("removeMapping")}
          </button>
          {mapped && (
            <p className="bc-summary">
              {mapped.text.slice(0, 1500)}
              {mapped.text.length > 1500 ? "…" : ""}
            </p>
          )}
        </div>
      ) : (
        <p className="bc-hint">{t("selectChapter")}</p>
      )}
    </div>
  );
}
