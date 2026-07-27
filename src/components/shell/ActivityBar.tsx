import type { ReactNode } from "react";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  /** Which panel the sidebar shows. */
  sidebarView: "explorer" | "search";
  onShowSearch: () => void;
  settingsOpen: boolean;
  onToggleSettings: () => void;
  consoleOpen: boolean;
  onToggleConsole: () => void;
};

function IconBtn({
  active, title, onClick, children,
}: { active?: boolean; title: string; onClick: () => void; children: ReactNode }) {
  return (
    <button className={`act-btn ${active ? "active" : ""}`} title={title} onClick={onClick}>
      {children}
    </button>
  );
}

/** Far-left icon rail: toggles the explorer, console, and settings. */
export function ActivityBar({
  t, sidebarOpen, onToggleSidebar, sidebarView, onShowSearch,
  settingsOpen, onToggleSettings, consoleOpen, onToggleConsole,
}: Props) {
  return (
    <nav className="activitybar">
      <IconBtn active={sidebarOpen && sidebarView === "explorer" && !settingsOpen} title={t("activity.explorer")} onClick={onToggleSidebar}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
          <path d="M3 5.5A1.5 1.5 0 0 1 4.5 4h4l2 2.5h7A1.5 1.5 0 0 1 19 8v9.5A1.5 1.5 0 0 1 17.5 19h-13A1.5 1.5 0 0 1 3 17.5z" />
        </svg>
      </IconBtn>
      <IconBtn active={sidebarOpen && sidebarView === "search" && !settingsOpen} title={t("activity.search")} onClick={onShowSearch}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="10.5" cy="10.5" r="6" />
          <path d="M15 15l4.5 4.5" />
        </svg>
      </IconBtn>
      <IconBtn active={consoleOpen} title={t("activity.console")} onClick={onToggleConsole}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
          <rect x="3" y="4.5" width="18" height="15" rx="1.6" />
          <path d="M7 9l3 3-3 3M13 15h4" />
        </svg>
      </IconBtn>
      <div className="spacer" />
      <IconBtn active={settingsOpen} title={t("activity.settings")} onClick={onToggleSettings}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 13.5a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 1 1-4 0v-.09A1.7 1.7 0 0 0 8.5 19.4a1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 1 1 0-4h.09A1.7 1.7 0 0 0 4.6 8.5a1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34H9a1.7 1.7 0 0 0 1-1.56V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1 1.56 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87V9a1.7 1.7 0 0 0 1.56 1H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.51 1.03z" />
        </svg>
      </IconBtn>
    </nav>
  );
}
