import { MARKER_GLYPHS, MARKER_ORDER, type MarkerId } from "../lib/chapters";
import { Modal } from "./common/Modal";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  onClose: () => void;
};

/** Markers that mean something is wrong get a warning colour, as in the tree. */
const FLAGGED: MarkerId[] = ["failed", "issues"];

/** Badges shown in the translation pane's header, beside the chapter title. */
const BADGES = ["reference", "manual", "queued", "translating", "failed", "issues", "pictures"] as const;

/** Help → Markers: what every glyph and badge in the UI stands for. */
export function Legend({ t, onClose }: Props) {
  return (
    <Modal className="legend" onClose={onClose}>
      <div className="legend-head">
        <div className="worktitle">{t("legend.title")}</div>
        <button className="icon" title={t("about.close")} onClick={onClose}>×</button>
      </div>

      <div className="legend-section">{t("legend.tree")}</div>
      <dl className="legend-rows">
        {MARKER_ORDER.map((id) => (
          <div className="legend-row" key={id}>
            <dt className={`legend-glyph ${FLAGGED.includes(id) ? "flagged" : ""}`}>{MARKER_GLYPHS[id]}</dt>
            <dd>
              <span className="legend-name">{t(`legend.marker.${id}`)}</span>
              <span className="legend-desc">{t(`legend.markerDesc.${id}`)}</span>
            </dd>
          </div>
        ))}
      </dl>

      <div className="legend-section">{t("legend.badges")}</div>
      <dl className="legend-rows">
        {BADGES.map((id) => (
          <div className="legend-row" key={id}>
            <dt className="legend-badge">
              <span className={`ref-badge ${id === "failed" ? "state-failed" : id === "issues" ? "state-issues" : ""}`}>
                {t(`legend.badge.${id}`)}
              </span>
            </dt>
            <dd>
              <span className="legend-desc">{t(`legend.badgeDesc.${id}`)}</span>
            </dd>
          </div>
        ))}
      </dl>
    </Modal>
  );
}
