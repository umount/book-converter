import { AppIcon } from "./common/AppIcon";

export type MenuId = "file" | "edit" | "view" | "help";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  menu: MenuId | null;
  setMenu: (m: MenuId | null) => void;
  busy: string | null;
  canExport: boolean;
  hasActive: boolean;
  onOpenBook: () => void;
  onOpenReference: () => void;
  onOpenProject: () => void;
  onSaveProject: () => void;
  onExport: (fmt: "fb2" | "epub" | "pdf" | "txt") => void;
  onFind: () => void;
  onReplace: () => void;
  onSearchBook: () => void;
  onToggleSidebar: () => void;
  onShowBothPanes: () => void;
  onToggleHighlight: () => void;
  onToggleConsole: () => void;
  onOpenCommandPalette: () => void;
  onOpenSettings: () => void;
  onOpenLegend: () => void;
  onOpenAbout: () => void;
};

export function Menubar({
  t, menu, setMenu, busy, canExport, hasActive,
  onOpenBook, onOpenReference, onOpenProject, onSaveProject, onExport, onFind, onReplace, onSearchBook,
  onToggleSidebar, onShowBothPanes, onToggleHighlight, onToggleConsole,
  onOpenCommandPalette, onOpenSettings, onOpenLegend, onOpenAbout,
}: Props) {
  return (
    <>
      <header className="menubar">
        <AppIcon className="brand-mark" />
        <div className="menuitem" onClick={() => setMenu(menu === "file" ? null : "file")}>
          {t("menu.file")}
          {menu === "file" && (
            <div className="dropdown" onClick={(e) => e.stopPropagation()}>
              <div className="mi" onClick={onOpenBook}>{t("file.openBook")}</div>
              <div className={`mi ${!hasActive ? "disabled" : ""}`} onClick={() => hasActive && onOpenReference()}>{t("file.openReference")}</div>
              <div className="sep" />
              <div className="mi" onClick={onOpenProject}>{t("file.openProject")}</div>
              <div className={`mi ${!hasActive ? "disabled" : ""}`} onClick={() => hasActive && onSaveProject()}>{t("file.saveProject")}</div>
              <div className="sep" />
              <div className="mi-label">{t("file.exportAs")}</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && onExport("fb2")}>FB2</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && onExport("epub")}>EPUB</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && onExport("pdf")}>PDF</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && onExport("txt")}>TXT</div>
            </div>
          )}
        </div>
        <div className="menuitem" onClick={() => setMenu(menu === "edit" ? null : "edit")}>
          {t("menu.edit")}
          {menu === "edit" && (
            <div className="dropdown" onClick={(e) => e.stopPropagation()}>
              <div className={`mi ${!hasActive ? "disabled" : ""}`} onClick={() => hasActive && onFind()}>
                {t("edit.find")}<span className="mi-key">⌘F</span>
              </div>
              <div className={`mi ${!hasActive ? "disabled" : ""}`} onClick={() => hasActive && onReplace()}>
                {t("edit.replace")}<span className="mi-key">⌘H</span>
              </div>
              <div className="sep" />
              <div className={`mi ${!hasActive ? "disabled" : ""}`} onClick={() => hasActive && onSearchBook()}>
                {t("edit.searchBook")}<span className="mi-key">⌘⇧F</span>
              </div>
            </div>
          )}
        </div>
        <div className="menuitem" onClick={() => setMenu(menu === "view" ? null : "view")}>
          {t("menu.view")}
          {menu === "view" && (
            <div className="dropdown" onClick={(e) => e.stopPropagation()}>
              <div className="mi" onClick={() => { onOpenCommandPalette(); setMenu(null); }}>{t("view.commandPalette")}<span className="mi-key">⌘P</span></div>
              <div className="sep" />
              <div className="mi" onClick={() => { onToggleSidebar(); setMenu(null); }}>{t("view.toggleSidebar")}<span className="mi-key">⌘B</span></div>
              <div className="mi" onClick={() => { onShowBothPanes(); setMenu(null); }}>{t("view.showBothPanes")}</div>
              <div className="mi" onClick={() => { onToggleHighlight(); setMenu(null); }}>{t("view.toggleHighlight")}</div>
              <div className="mi" onClick={() => { onToggleConsole(); setMenu(null); }}>{t("view.toggleConsole")}<span className="mi-key">⌘J</span></div>
            </div>
          )}
        </div>
        <div className="menuitem" onClick={onOpenSettings}>{t("menu.settings")}</div>
        <div className="menuitem" onClick={() => setMenu(menu === "help" ? null : "help")}>
          {t("menu.help")}
          {menu === "help" && (
            <div className="dropdown" onClick={(e) => e.stopPropagation()}>
              <div className="mi" onClick={() => { onOpenLegend(); setMenu(null); }}>{t("help.legend")}</div>
              <div className="sep" />
              <div className="mi" onClick={() => { onOpenAbout(); setMenu(null); }}>{t("help.about")}</div>
            </div>
          )}
        </div>
        <div className="menu-spacer" />
        {busy && <span className="busy-inline">{busy}</span>}
      </header>
      {busy && <div className="loadbar" />}
      {menu && <div className="menu-backdrop" onClick={() => setMenu(null)} />}
    </>
  );
}
