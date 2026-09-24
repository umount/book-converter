import type { MangaPageView } from "../../shared/contracts/generated";
import type { T } from "../../app/strings";
export function RegionInspector({
  view,
  selected,
  onSelect,
  onClose,
  t,
}: {
  view: MangaPageView | null;
  selected: string | null;
  onSelect: (id: string) => void;
  onClose: () => void;
  t: T;
}) {
  const region = view?.regions.find((r) => r.id === selected);
  return (
    <section className="bc-region-inspector" aria-label={t("regions")}>
      <header>
        <strong>
          {t("regions")}
          {view ? ` · ${view.regions.length}` : ""}
        </strong>
        <button onClick={onClose} aria-label={t("close")}>
          ×
        </button>
      </header>
      {!view ? (
        <p>{t("loading")}</p>
      ) : (
        <>
          {!view.recognition ? (
            <p className="bc-hint">{t("notRecognized")}</p>
          ) : !view.recognition.current ? (
            <p className="bc-warning">{t("recognitionStale")}</p>
          ) : view.recognition.needsReview ? (
            <p className="bc-warning">{t("review")}</p>
          ) : view.regions.length === 0 ? (
            <p className="bc-hint">{t("noRecognizedText")}</p>
          ) : null}
          <div className="bc-region-list">
            {view.regions.map((r) => (
              <button
                key={r.id}
                aria-pressed={selected === r.id}
                onClick={() => onSelect(r.id)}
              >
                <span>{r.readingOrder + 1}</span>
                <span>{r.sourceText}</span>
              </button>
            ))}
          </div>
          {region && (
            <div className="bc-region-text">
              <p lang="und">{region.sourceText}</p>
              <p className={region.translatedText ? undefined : "bc-hint"}>
                {region.translatedText ?? t("translationNotReady")}
              </p>
              {(region.sourceManual || region.translationManual) && (
                <small className="bc-hint">{t("manualRegionText")}</small>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
