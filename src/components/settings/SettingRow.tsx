import type { ReactNode } from "react";

type Props = {
  label: string;
  desc?: string;
  locked?: boolean;
  lockedNote?: string;
  children: ReactNode;
};

/** One settings entry: label + description on the left, a control on the right. */
export function SettingRow({ label, desc, locked, lockedNote, children }: Props) {
  return (
    <div className="setting-row">
      <div className="setting-info">
        <div className="setting-label">{label}</div>
        {desc && <div className="setting-desc">{desc}</div>}
        {locked && lockedNote && <div className="setting-locked">{lockedNote}</div>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}
