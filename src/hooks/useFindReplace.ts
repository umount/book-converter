import { useRef, useState } from "react";

export type FindScope = "chapter" | "book";
export type FindMode = "find" | "replace";

/** State for the reader's find/replace bar (⌘F / ⌘H). */
export function useFindReplace() {
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<FindMode>("find");
  const [query, setQuery] = useState("");
  const [replacement, setReplacement] = useState("");
  const [matchCase, setMatchCase] = useState(false);
  const [wholeWord, setWholeWord] = useState(false);
  const [regex, setRegex] = useState(false);
  const [scope, setScope] = useState<FindScope>("chapter");
  const [current, setCurrent] = useState(0);
  // Bumped on every open so the bar refocuses and reselects, like in an IDE
  // where hitting ⌘F again grabs the field back.
  const [focusToken, setFocusToken] = useState(0);
  const seedRef = useRef<() => string>(() => "");

  /** Register how to read the editor's current selection (used to seed the query). */
  function setSeeder(fn: () => string) {
    seedRef.current = fn;
  }

  function openBar(m: FindMode) {
    const selected = seedRef.current().trim();
    // A selection wins over the previous query, as in VS Code / Cursor.
    if (selected && !selected.includes("\n")) {
      setQuery(selected);
      setCurrent(0);
    }
    setMode(m);
    setOpen(true);
    setFocusToken((n) => n + 1);
  }
  function close() {
    setOpen(false);
  }

  return {
    open, setOpen, openBar, close,
    mode, setMode,
    query, setQuery,
    replacement, setReplacement,
    matchCase, setMatchCase,
    wholeWord, setWholeWord,
    regex, setRegex,
    scope, setScope,
    current, setCurrent,
    focusToken, setSeeder,
  };
}

export type FindApi = ReturnType<typeof useFindReplace>;
