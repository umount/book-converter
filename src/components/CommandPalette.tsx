import { useEffect, useMemo, useRef, useState } from "react";
import { chapterLabel, chapterMatches } from "../lib/chapters";
import type { ChapterRow } from "../types";

export type Command = { id: string; label: string; hint?: string; run: () => void };

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  open: boolean;
  onClose: () => void;
  commands: Command[];
  chapters: ChapterRow[];
  onOpenChapter: (idx: number) => void;
};

type Row =
  | { kind: "command"; cmd: Command }
  | { kind: "chapter"; chapter: ChapterRow };

const MAX_CHAPTERS = 50;

/** Quick-open palette (⌘P): jump to a chapter by number/title or run an action. */
export function CommandPalette({ t, open, onClose, commands, chapters, onOpenChapter }: Props) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQ("");
      setSel(0);
      // Focus after the element mounts.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const rows = useMemo<Row[]>(() => {
    const query = q.trim().toLowerCase();
    const cmds = (query ? commands.filter((c) => c.label.toLowerCase().includes(query)) : commands)
      .map((cmd): Row => ({ kind: "command", cmd }));
    // Chapters only when searching (the full list lives in the explorer).
    const chs: Row[] = query
      ? chapters.filter((c) => chapterMatches(c, query)).slice(0, MAX_CHAPTERS).map((chapter) => ({ kind: "chapter", chapter }))
      : [];
    return [...cmds, ...chs];
  }, [q, commands, chapters]);

  useEffect(() => {
    if (sel >= rows.length) setSel(0);
  }, [rows.length, sel]);

  if (!open) return null;

  function run(row: Row) {
    onClose();
    if (row.kind === "command") row.cmd.run();
    else onOpenChapter(row.chapter.idx);
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); onClose(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(rows.length - 1, s + 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
    else if (e.key === "Enter") { e.preventDefault(); if (rows[sel]) run(rows[sel]); }
  }

  return (
    <div className="palette-backdrop" onClick={onClose}>
      <div className="palette" onClick={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="palette-input"
          placeholder={t("palette.placeholder")}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="palette-list">
          {rows.length === 0 && <div className="palette-empty">{t("palette.empty")}</div>}
          {rows.map((row, i) => {
            const active = i === sel;
            if (row.kind === "command") {
              return (
                <div key={`c-${row.cmd.id}`} className={`palette-row ${active ? "active" : ""}`}
                  onMouseEnter={() => setSel(i)} onClick={() => run(row)}>
                  <span className="palette-label">{row.cmd.label}</span>
                  {row.cmd.hint && <span className="palette-hint">{row.cmd.hint}</span>}
                </div>
              );
            }
            const c = row.chapter;
            return (
              <div key={`h-${c.idx}`} className={`palette-row ${active ? "active" : ""}`}
                onMouseEnter={() => setSel(i)} onClick={() => run(row)}>
                {c.number != null && <span className="palette-num">#{c.number}</span>}
                <span className="palette-label">{chapterLabel(c)}</span>
                <span className="palette-hint">{t("palette.chapter")}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
