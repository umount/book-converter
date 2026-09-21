import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { CallFn } from "../api";

export type AssistantStatus = "idle" | "running" | "awaiting_confirm" | "error";

export type Invalidate =
  | "progress"
  | "chapters"
  | "open_chapter"
  | "glossary"
  | "book_details"
  | "reference";

export type AssistantMessage = {
  id: number;
  role: "user" | "assistant" | "tool" | "system_note";
  content: string;
  tool_name?: string | null;
  tool_call_id?: string | null;
};

export type AssistantConfirm = {
  id: string;
  tool: string;
  args: string;
  heavy: boolean;
};

type Opts = {
  call: CallFn;
  activeId: string;
  enabled: boolean;
  onInvalidated: (areas: Invalidate[]) => void | Promise<void>;
};

type HistoryRow = {
  id: number;
  role: string;
  content: string;
  tool_name: string | null;
  tool_call_id: string | null;
};

type AssistantStateDto = {
  running: boolean;
  pending: AssistantConfirm | null;
};

/** Project assistant chat: history, send loop, confirm / cancel. */
export function useAssistant({ call, activeId, enabled, onInvalidated }: Opts) {
  const [messages, setMessages] = useState<AssistantMessage[]>([]);
  const [status, setStatus] = useState<AssistantStatus>("idle");
  const [pendingConfirm, setPendingConfirm] = useState<AssistantConfirm | null>(null);
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;
  const onInvalidatedRef = useRef(onInvalidated);
  onInvalidatedRef.current = onInvalidated;
  const invalidatedRef = useRef<Set<Invalidate>>(new Set());

  async function loadHistory(projectId: string = activeIdRef.current) {
    if (!projectId) {
      setMessages([]);
      return;
    }
    const rows = await call<HistoryRow[]>("assistant_history", { projectId });
    if (activeIdRef.current !== projectId) return;
    setMessages(
      (rows ?? []).map((r) => ({
        id: r.id,
        role: r.role as AssistantMessage["role"],
        content: r.content,
        tool_name: r.tool_name,
        tool_call_id: r.tool_call_id,
      })),
    );
  }

  async function restoreState(projectId: string) {
    if (!projectId || !enabled) {
      setStatus("idle");
      setPendingConfirm(null);
      return;
    }
    const st = await call<AssistantStateDto>("assistant_state", { projectId });
    if (activeIdRef.current !== projectId) return;
    if (st?.pending) {
      setPendingConfirm(st.pending);
      setStatus("awaiting_confirm");
    } else if (st?.running) {
      setPendingConfirm(null);
      setStatus("running");
    } else {
      setPendingConfirm(null);
      setStatus("idle");
    }
  }

  useEffect(() => {
    invalidatedRef.current = new Set();
    if (enabled) {
      void loadHistory(activeId);
      void restoreState(activeId);
    } else {
      setMessages([]);
      setStatus("idle");
      setPendingConfirm(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId, enabled]);

  useEffect(() => {
    let unsubs: Array<() => void> = [];
    (async () => {
      unsubs = [
        await listen<{
          project: string;
          role: string;
          content: string;
          tool_name?: string;
          phase?: string;
          ok?: boolean;
          invalidates?: Invalidate[];
        }>("assistant_step", (e) => {
          if (e.payload.project !== activeIdRef.current) return;
          const role = e.payload.role as AssistantMessage["role"];
          setMessages((ms) => [
            ...ms,
            {
              id: Date.now() + Math.random(),
              role,
              content: e.payload.content,
              tool_name: e.payload.tool_name ?? null,
            },
          ]);
          if (e.payload.phase === "result" && e.payload.ok && e.payload.invalidates) {
            for (const area of e.payload.invalidates) invalidatedRef.current.add(area);
          }
          setStatus((s) => (s === "awaiting_confirm" ? s : "running"));
        }),
        await listen<{
          project: string; id: string; tool: string; args: string; heavy: boolean;
        }>("assistant_need_confirm", (e) => {
          if (e.payload.project !== activeIdRef.current) return;
          setPendingConfirm({
            id: e.payload.id,
            tool: e.payload.tool,
            args: e.payload.args,
            heavy: e.payload.heavy,
          });
          setStatus("awaiting_confirm");
        }),
        await listen<{ project: string; id: string }>("assistant_confirm_expired", (e) => {
          if (e.payload.project !== activeIdRef.current) return;
          setPendingConfirm(null);
          setStatus("running");
        }),
        await listen<{ project: string }>("assistant_done", (e) => {
          if (e.payload.project !== activeIdRef.current) return;
          setPendingConfirm(null);
          setStatus("idle");
          const areas = [...invalidatedRef.current];
          invalidatedRef.current = new Set();
          void loadHistory(e.payload.project);
          if (areas.length) void onInvalidatedRef.current(areas);
        }),
        await listen<{ project: string; message: string }>("assistant_error", (e) => {
          if (e.payload.project !== activeIdRef.current) return;
          setPendingConfirm(null);
          setStatus("error");
          void loadHistory(e.payload.project);
        }),
      ].map((u) => () => { void u(); });
    })();
    return () => unsubs.forEach((u) => u());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function sendWithChapter(text: string, openChapter: number | null) {
    if (!enabled || !activeId) return;
    invalidatedRef.current = new Set();
    setStatus("running");
    setMessages((ms) => [
      ...ms,
      { id: Date.now(), role: "user", content: text },
    ]);
    const ok = await call("assistant_send", { projectId: activeId, message: text, openChapter });
    if (ok === undefined) {
      void loadHistory(activeId);
      void restoreState(activeId);
    }
  }

  async function clear() {
    if (!activeId) return;
    await call("assistant_cancel", { projectId: activeId });
    await call("assistant_clear", { projectId: activeId });
    setMessages([]);
    setPendingConfirm(null);
    setStatus("idle");
  }

  async function approve(confirmId: string) {
    const ok = await call("assistant_approve", { projectId: activeId, confirmId, approved: true });
    if (ok === undefined) return;
    setPendingConfirm(null);
    setStatus("running");
  }

  async function deny(confirmId: string) {
    const ok = await call("assistant_approve", { projectId: activeId, confirmId, approved: false });
    if (ok === undefined) return;
    setPendingConfirm(null);
    setStatus("running");
  }

  async function cancel() {
    await call("assistant_cancel", { projectId: activeId });
    setPendingConfirm(null);
    setStatus("idle");
  }

  return {
    messages, status, pendingConfirm,
    sendWithChapter, clear, approve, deny, cancel, loadHistory,
  };
}
