import { useState } from "react";
import { Modal } from "../shared/ui/Modal";
import type { T } from "./strings";
export type Command = {
  id: string;
  label: string;
  run: () => void;
};
export function CommandPalette({
  commands,
  close,
  t,
}: {
  commands: Command[];
  close: () => void;
  t: T;
}) {
  const [query, setQuery] = useState("");
  const visible = commands.filter((c) =>
    c.label.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
  );
  function choose(command: Command) {
    close();
    command.run();
  }
  return (
    <Modal title={t("commands")} closeLabel={t("close")} onClose={close}>
      <div className="bc-dialog-body">
        <input
          autoFocus
          aria-label={t("commands")}
          placeholder={t("find")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && visible[0]) {
              e.preventDefault();
              choose(visible[0]);
            }
          }}
        />
        <div className="bc-command-list">
          {visible.map((command) => (
            <button key={command.id} onClick={() => choose(command)}>
              {command.label}
            </button>
          ))}
          {!visible.length && <p>{t("noMatches")}</p>}
        </div>
      </div>
    </Modal>
  );
}
