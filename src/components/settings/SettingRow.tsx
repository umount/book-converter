import type { ReactNode } from "react";

type Props = {
  label: string;
  desc?: string;
  locked?: boolean;
  lockedNote?: string;
  /**
   * Centre the control against the whole row instead of aligning it with the
   * label. Fields are tall enough to read as aligned on their own; a checkbox
   * is not, and sits visibly high next to a two-line description.
   */
  centered?: boolean;
  children: ReactNode;
};

/** One settings entry: label + description on the left, a control on the right. */
export function SettingRow({ label, desc, locked, lockedNote, centered, children }: Props) {
  return (
    <div className="setting-row">
      <div className="setting-info">
        <div className="setting-label">{label}</div>
        {desc && <div className="setting-desc">{desc}</div>}
        {locked && lockedNote && <div className="setting-locked">{lockedNote}</div>}
      </div>
      <div className={`setting-control ${centered ? "centered" : ""}`}>{children}</div>
    </div>
  );
}
