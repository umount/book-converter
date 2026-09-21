import { useEffect, useRef, useState } from "react";
import type {
  AssistantConfirm,
  AssistantMessage,
  AssistantStatus,
} from "../../hooks/useAssistant";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  open: boolean;
  width: number;
  enabled: boolean;
  messages: AssistantMessage[];
  status: AssistantStatus;
  pendingConfirm: AssistantConfirm | null;
  confirmExpired: boolean;
  onClose: () => void;
  onClear: () => void;
  onSend: (text: string) => void;
  onApprove: (confirmId: string) => void;
  onDeny: (confirmId: string) => void;
  onCancel: () => void;
};

/** Right-side project assistant dock (Cursor-style chat). */
export function AssistantPanel({
  t, open, width, enabled, messages, status, pendingConfirm, confirmExpired,
  onClose, onClear, onSend, onApprove, onDeny, onCancel,
}: Props) {
  const [draft, setDraft] = useState("");
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    listRef.current?.scrollTo(0, listRef.current.scrollHeight);
  }, [messages, status, pendingConfirm, confirmExpired]);

  if (!open) return null;

  function submit() {
    const text = draft.trim();
    if (!text || !enabled || status === "running" || status === "awaiting_confirm") return;
    setDraft("");
    onSend(text);
  }

  const statusLabel =
    status === "running" ? t("assistant.status.running")
    : status === "awaiting_confirm" ? t("assistant.status.awaitingConfirm")
    : status === "error" ? t("assistant.status.error")
    : null;

  return (
    <aside className="assistant" style={{ width }} aria-label={t("assistant.title")}>
      <div className="assistant-head">
        <span className="assistant-title">{t("assistant.title")}</span>
        <div className="assistant-actions">
          <button className="icon" title={t("assistant.clear")} onClick={onClear} disabled={!enabled}>
            ⌫
          </button>
          <button className="icon" title={t("assistant.close")} onClick={onClose}>×</button>
        </div>
      </div>

      {!enabled ? (
        <div className="assistant-empty muted">{t("assistant.needProject")}</div>
      ) : (
        <>
          <div className="assistant-list" ref={listRef}>
            {messages.length === 0 && (
              <div className="assistant-empty muted">{t("assistant.empty")}</div>
            )}
            {messages.map((m) => (
              <div key={m.id} className={`assistant-msg role-${m.role}`}>
                {m.role === "tool" ? (
                  <div className="assistant-tool">
                    <span className="assistant-tool-name">{m.tool_name || "tool"}</span>
                    <pre>{m.content}</pre>
                  </div>
                ) : (
                  <div className="assistant-bubble">{m.content}</div>
                )}
              </div>
            ))}
            {confirmExpired && !pendingConfirm && (
              <div className="assistant-confirm-warn">{t("assistant.confirm.expired")}</div>
            )}
            {pendingConfirm && (
              <div className="assistant-confirm">
                <div className="assistant-confirm-title">{t("assistant.confirm.title")}</div>
                <div className="assistant-confirm-tool">{pendingConfirm.tool}</div>
                <pre className="assistant-confirm-args">{pendingConfirm.args}</pre>
                {pendingConfirm.heavy && (
                  <div className="assistant-confirm-warn">{t("assistant.confirm.heavy")}</div>
                )}
                <div className="assistant-confirm-actions">
                  <button className="primary" onClick={() => onApprove(pendingConfirm.id)}>
                    {t("assistant.confirm.approve")}
                  </button>
                  <button onClick={() => onDeny(pendingConfirm.id)}>
                    {t("assistant.confirm.deny")}
                  </button>
                </div>
              </div>
            )}
          </div>

          {statusLabel && (
            <div className="assistant-status">
              <span className="spinner tiny" />
              <span>{statusLabel}</span>
              { (status === "running" || status === "awaiting_confirm") && (
                <button className="linkish" onClick={onCancel}>{t("assistant.cancel")}</button>
              )}
            </div>
          )}

          <div className="assistant-input">
            <textarea
              rows={3}
              value={draft}
              placeholder={t("assistant.placeholder")}
              disabled={status === "running" || status === "awaiting_confirm"}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  submit();
                }
              }}
            />
            <button
              className="primary"
              disabled={!draft.trim() || status === "running" || status === "awaiting_confirm"}
              onClick={submit}
            >
              {t("assistant.send")}
            </button>
          </div>
        </>
      )}
    </aside>
  );
}
