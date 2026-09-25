import { useEffect, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type { MangaPageView } from "../../shared/contracts/generated";

export function usePageView(projectId: string, pageId: string | undefined) {
  const [version, setVersion] = useState(0);
  const [view, setView] = useState<MangaPageView | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    let generation = 0;
    setView(null);
    setError(null);
    if (!pageId) return;
    const refresh = async () => {
      const request = ++generation;
      try {
        const value = await projectApi.mangaPage({ projectId, pageId });
        if (alive && request === generation) {
          setView(value);
          setError(null);
        }
      } catch (e) {
        if (alive && request === generation) setError(e);
      }
    };
    void refresh();
    const subscription = projectApi.subscribe(projectId, (event) => {
      if (
        event.type === "job.updated" ||
        (event.type === "entity.changed" && event.payload.id === pageId)
      )
        void refresh();
    });
    void subscription.ready.catch((e) => {
      if (alive) setError(e);
    });
    return () => {
      alive = false;
      subscription.dispose();
    };
  }, [projectId, pageId, version]);
  return {
    refresh: () => setVersion((v) => v + 1),
    view: view?.page.id === pageId ? view : null,
    error,
  };
}
