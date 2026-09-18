import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { CallFn } from "../api";

export type AssistantStatus = "idle" | "running" | "awaiting_confirm" | "error";

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
  /** Refresh UI after a mutating tool succeeds. */
  onMutated: (tools: string[]) => void | Promise<void>;
};

type HistoryRow = {
  id: number;
  role: string;
  content: string;
  tool_name: string | null;
  tool_call_id: string | null;
};

/** Project assistant chat: history, send loop, confirm / cancel. */
export function useAssistant({ call, activeId, enabled, onMutated }: Opts) {
  const [messages, setMessages] = useState<AssistantMessage[]>([]);
  const [status, setStatus] = useState<AssistantStatus>("idle");
  const [pendingConfirm, setPendingConfirm] = useState<AssistantConfirm | null>(null);
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;
  const onMutatedRef = useRef(onMutated);
  onMutatedRef.current = onMutated;
  const mutatedToolsRef = useRef<string[]>([]);

  async function loadHistory(projectId: string = activeId) {
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

  useEffect(() => {
    setStatus("idle");
    setPendingConfirm(null);
    mutatedToolsRef.current = [];
    if (enabled) void loadHistory(activeId);
    else setMessages([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId, enabled]);

  useEffect(() => {
    let unsubs: Array<() => void> = [];
    (async () => {
      unsubs = [
        await listen<{ project: string; role: string; content: string; tool_name?: string }>(
          "assistant_step",
          (e) => {
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
            if (role === "tool" && e.payload.tool_name) {
              mutatedToolsRef.current.push(e.payload.tool_name);
            }
            setStatus((s) => (s === "awaiting_confirm" ? s : "running"));
          },
        ),
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
        await listen<{ project: string }>("assistant_done", (e) => {
          if (e.payload.project !== activeIdRef.current) return;
          setPendingConfirm(null);
          setStatus("idle");
          const tools = mutatedToolsRef.current.splice(0);
          void loadHistory();
          if (tools.length) void onMutatedRef.current(tools);
        }),
        await listen<{ project: string; message: string }>("assistant_error", (e) => {
          if (e.payload.project !== activeIdRef.current) return;
          setPendingConfirm(null);
          setStatus("error");
          setMessages((ms) => [
            ...ms,
            {
              id: Date.now(),
              role: "system_note",
              content: e.payload.message,
            },
          ]);
        }),
      ].map((u) => () => { void u(); });
    })();
    return () => unsubs.forEach((u) => u());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function send(text: string) {
    if (!enabled || !activeId) return;
    mutatedToolsRef.current = [];
    setStatus("running");
    setMessages((ms) => [
      ...ms,
      { id: Date.now(), role: "user", content: text },
    ]);
    const ok = await call("assistant_send", {
      projectId: activeId,
      message: text,
      openChapter: null as number | null,
    });
    if (ok === undefined) setStatus("error");
  }

  async function sendWithChapter(text: string, openChapter: number | null) {
    if (!enabled || !activeId) return;
    mutatedToolsRef.current = [];
    setStatus("running");
    setMessages((ms) => [
      ...ms,
      { id: Date.now(), role: "user", content: text },
    ]);
    const ok = await call("assistant_send", {
      projectId: activeId,
      message: text,
      openChapter,
    });
    if (ok === undefined) setStatus("error");
  }

  async function clear() {
    if (!activeId) return;
    await call("assistant_clear", { projectId: activeId });
    setMessages([]);
    setPendingConfirm(null);
    setStatus("idle");
  }

  async function approve(confirmId: string) {
    await call("assistant_approve", { projectId: activeId, confirmId, approved: true });
    setPendingConfirm(null);
    setStatus("running");
  }

  async function deny(confirmId: string) {
    await call("assistant_approve", { projectId: activeId, confirmId, approved: false });
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
    send, sendWithChapter, clear, approve, deny, cancel, loadHistory,
  };
}
