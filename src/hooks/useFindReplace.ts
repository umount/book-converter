import { useState } from "react";

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
  const [scope, setScope] = useState<FindScope>("chapter");
  const [current, setCurrent] = useState(0);

  function openBar(m: FindMode) {
    setMode(m);
    setOpen(true);
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
    scope, setScope,
    current, setCurrent,
  };
}
