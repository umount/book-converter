import type { ViewId } from "../../types";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  view: ViewId;
  setView: (v: ViewId) => void;
  glossaryCount: number;
};

const TABS: { id: ViewId; key: string }[] = [
  { id: "overview", key: "nav.overview" },
  { id: "reader", key: "nav.translation" },
  { id: "glossary", key: "nav.glossary" },
];

/** Editor tab strip. In P0 the tabs are the three views; chapter tabs land in P1. */
export function TabBar({ t, view, setView, glossaryCount }: Props) {
  return (
    <div className="tabbar" role="tablist">
      {TABS.map((tab) => (
        <div
          key={tab.id}
          role="tab"
          aria-selected={view === tab.id}
          className={`tab ${view === tab.id ? "active" : ""}`}
          onClick={() => setView(tab.id)}
        >
          {t(tab.key)}
          {tab.id === "glossary" && <span className="tab-badge">{glossaryCount}</span>}
        </div>
      ))}
    </div>
  );
}
