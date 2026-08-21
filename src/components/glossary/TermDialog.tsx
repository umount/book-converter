import { useEffect, useRef, useState } from "react";
import { TERM_KINDS, type Term } from "../../types";
import { Modal } from "../common/Modal";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  /** The term being edited, or null when adding a new one. */
  term: Term | null;
  /** True when the book already has translated text the rename would affect. */
  hasTranslation: boolean;
  onSave: (next: Term) => void | Promise<void>;
  onClose: () => void;
};

/**
 * Add or edit one glossary term.
 *
 * The glossary table used to be editable in place and saved on blur, so a
 * mistaken click changed a term, and a changed rendering queued a rewrite of
 * every affected paragraph in the book. Nothing is written until Save is
 * pressed here, and the consequence of changing a rendering is stated before
 * it is.
 */
export function TermDialog({ t, term, hasTranslation, onSave, onClose }: Props) {
  const [source, setSource] = useState(term?.source ?? "");
  const [target, setTarget] = useState(term?.target ?? "");
  const [kind, setKind] = useState(term?.kind ?? "person");
  const [pinned, setPinned] = useState(term?.pinned ?? true);
  const [busy, setBusy] = useState(false);

  const firstRef = useRef<HTMLInputElement>(null);
  useEffect(() => firstRef.current?.focus(), []);

  const trimmedSource = source.trim();
  const trimmedTarget = target.trim();
  const valid = !!trimmedSource && !!trimmedTarget;
  const renamed = !!term && trimmedTarget !== term.target;
  const resourced = !!term && trimmedSource !== term.source;

  async function save() {
    if (!valid || busy) return;
    setBusy(true);
    try {
      await onSave({
        source: trimmedSource,
        target: trimmedTarget,
        kind,
        frequency: term?.frequency ?? 1,
        pinned,
      });
      onClose();
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal className="term-dialog" onClose={onClose}>
      <h3>{term ? t("term.editTitle") : t("term.newTitle")}</h3>

      <label className="term-field">
        <span>{t("term.source")}</span>
        <input
          ref={firstRef}
          value={source}
          onChange={(e) => setSource(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void save()}
          placeholder={t("glossary.addSource")}
        />
      </label>

      <label className="term-field">
        <span>{t("term.target")}</span>
        <input
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void save()}
          placeholder={t("glossary.addTranslation")}
        />
      </label>

      <label className="term-field">
        <span>{t("term.kind")}</span>
        <select value={kind} onChange={(e) => setKind(e.target.value)}>
          {TERM_KINDS.map((k) => (
            <option key={k} value={k}>{t(`kind.${k}`)}</option>
          ))}
        </select>
      </label>

      <label className="term-check">
        <input type="checkbox" checked={pinned} onChange={(e) => setPinned(e.target.checked)} />
        <span>{t("term.pinned")}</span>
      </label>

      {renamed && hasTranslation && (
        <p className="term-note">{t("term.renameNote", { old: term!.target, new: trimmedTarget })}</p>
      )}
      {resourced && <p className="term-note">{t("term.sourceNote")}</p>}

      <div className="term-actions">
        <button className="ghost" onClick={onClose} disabled={busy}>{t("term.cancel")}</button>
        <button className="primary" onClick={() => void save()} disabled={!valid || busy}>
          {busy ? t("reader.saving") : t("term.save")}
        </button>
      </div>
    </Modal>
  );
}
