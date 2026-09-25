import type { MangaPageView } from "../../shared/contracts/generated";
import type { T } from "../../app/strings";
export function RegionInspector({
  view,
  selected,
  onSelect,
  onClose,
  onChangeDirection,
  onApply,
  onDiscard,
  busy,
  changed,
  t,
}: {
  view: MangaPageView | null;
  selected: string | null;
  onSelect: (id: string) => void;
  onClose: () => void;
  onChangeDirection: (id: string, vertical: boolean) => void;
  onApply: () => void;
  onDiscard: () => void;
  busy: boolean;
  changed: boolean;
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
          <p className="bc-hint">{t("regionEditHint")}</p>
          <button disabled={busy || !view.regions.length} onClick={onApply}>
            {t(busy ? "processing" : "applyRegions")}
            {changed ? " *" : ""}
          </button>
          {changed && (
            <button disabled={busy} onClick={onDiscard}>
              {t("cancel")}
            </button>
          )}
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
              <label>
                {t("textDirection")}
                <select
                  disabled={busy}
                  value={region.vertical ? "vertical" : "horizontal"}
                  onChange={(e) =>
                    onChangeDirection(region.id, e.target.value === "vertical")
                  }
                >
                  <option value="horizontal">{t("horizontalText")}</option>
                  <option value="vertical">{t("verticalText")}</option>
                </select>
              </label>
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
