import type { ReactNode } from "react";

type Props = {
  id: string;
  title: string;
  extra?: ReactNode;
  children: ReactNode;
  collapsed: Record<string, boolean>;
  onToggle: (id: string) => void;
};

/** Collapsible panel used in overview/settings. */
export function Panel({ id, title, extra, children, collapsed, onToggle }: Props) {
  return (
    <section className="panel">
      <div className="panel-head" onClick={() => onToggle(id)}>
        <span className={`chevron ${collapsed[id] ? "closed" : ""}`}>▾</span>
        <span className="panel-title">{title}</span>
        <span className="panel-extra" onClick={(e) => e.stopPropagation()}>{extra}</span>
      </div>
      {!collapsed[id] && <div className="panel-body">{children}</div>}
    </section>
  );
}
