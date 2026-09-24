import { useEffect, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type { PageSummary } from "../../shared/contracts/generated";
import { assetUrl } from "../../shared/api/assets";
import { errorText, type T } from "../../app/strings";
export function MangaWorkspace({ projectId, t }: { projectId: string; t: T }) {
  const [pages, setPages] = useState<PageSummary[]>([]),
    [selected, setSelected] = useState(0),
    [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    void (async () => {
      let cursor: string | null = null;
      const all: PageSummary[] = [];
      do {
        const result = await projectApi.mangaPages({
          projectId,
          volumeId: null,
          cursor,
          limit: 200,
        });
        if (!alive) return;
        all.push(...result.items);
        cursor = result.nextCursor;
        setPages([...all]);
      } while (cursor);
    })().catch((e) => {
      if (alive) setError(e);
    });
    return () => {
      alive = false;
    };
  }, [projectId]);
  const page = pages[selected];
  return (
    <div className="bc-manga">
      <aside>
        <h2>{t("pages")}</h2>
        {pages.map((p, i) => (
          <button
            key={p.id}
            aria-current={i === selected ? "page" : undefined}
            onClick={() => setSelected(i)}
          >
            <img
              loading="lazy"
              src={assetUrl(projectId, p.originalAssetId)}
              alt=""
            />
            <span>
              {t("page")} {i + 1}
            </span>
          </button>
        ))}
      </aside>
      <div className="bc-manga-page">
        <p className="bc-warning">{t("mangaUnavailable")}</p>
        {error != null && (
          <p role="alert" className="bc-error">
            {errorText(error, t)}
          </p>
        )}
        {page ? (
          <img
            src={assetUrl(projectId, page.originalAssetId)}
            alt={`${t("page")} ${selected + 1}`}
          />
        ) : (
          <p>{t("noPages")}</p>
        )}
      </div>
    </div>
  );
}
