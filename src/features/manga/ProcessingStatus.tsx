import { useEffect, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type {
  MangaPreflight,
  MangaStage,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";

const stages: Record<MangaStage, Parameters<T>[0]> = {
  detection: "mangaDetection",
  recognition: "mangaRecognition",
  translation: "mangaTranslation",
  masks: "mangaMasks",
  inpainting: "mangaInpainting",
  lettering: "mangaLettering",
};
const reasons: Record<string, Parameters<T>[0]> = {
  mangaRecognitionProfileRequired: "mangaRecognitionProfileRequired",
  mangaTranslationProfileRequired: "mangaTranslationProfileRequired",
  mangaProfileKeyRequired: "mangaProfileKeyRequired",
  mangaProfileInvalid: "mangaProfileInvalid",
  mangaTranslationUnavailable: "mangaTranslationUnavailable",
  mangaMasksUnavailable: "mangaMasksUnavailable",
  mangaInpaintingUnavailable: "mangaInpaintingUnavailable",
  mangaLetteringUnavailable: "mangaLetteringUnavailable",
};

/** Opening this panel only checks local configuration. It never starts processing. */
export function ProcessingStatus({
  projectId,
  t,
}: {
  projectId: string;
  t: T;
}) {
  const [open, setOpen] = useState(false);
  const [version, setVersion] = useState(0);
  const [result, setResult] = useState<MangaPreflight | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    if (!open) return;
    let active = true;
    setResult(null);
    setError(null);
    void projectApi
      .mangaPreflight({ projectId })
      .then((value) => {
        if (active) setResult(value);
      })
      .catch((reason) => {
        if (active) setError(reason);
      });
    return () => {
      active = false;
    };
  }, [open, projectId, version]);
  return (
    <details
      className="bc-manga-preflight"
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>{t("mangaPreflight")}</summary>
      {open && (
        <>
          <p className="bc-hint">{t("mangaPreflightHint")}</p>
          {error != null ? (
            <p role="alert" className="bc-error">
              {errorText(error, t)}
            </p>
          ) : !result ? (
            <p role="status">{t("loading")}</p>
          ) : (
            <ul>
              {result.requirements.map((item) => (
                <li key={item.stage}>
                  <strong>{t(stages[item.stage])}</strong>
                  <span>
                    {t(
                      item.available
                        ? "mangaConfigured"
                        : (reasons[item.reasonKey ?? ""] ??
                            "mangaCapabilityUnavailable"),
                    )}
                  </span>
                </li>
              ))}
            </ul>
          )}
          <button onClick={() => setVersion((value) => value + 1)}>
            {t("refresh")}
          </button>
        </>
      )}
    </details>
  );
}
