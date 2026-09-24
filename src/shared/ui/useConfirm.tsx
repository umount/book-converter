import { useEffect, useRef, useState } from "react";
import type { T } from "../../app/strings";
import { Modal } from "./Modal";
type Request = {
  title: string;
  message: string;
  action: string;
  danger?: boolean;
};
export function useConfirm(t: T) {
  const [request, setRequest] = useState<Request | null>(null);
  const pending = useRef<((answer: boolean) => void) | null>(null);
  useEffect(
    () => () => {
      pending.current?.(false);
    },
    [],
  );
  function finish(answer: boolean) {
    const resolve = pending.current;
    pending.current = null;
    setRequest(null);
    resolve?.(answer);
  }
  return {
    confirm: (next: Request) =>
      new Promise<boolean>((resolve) => {
        pending.current?.(false);
        pending.current = resolve;
        setRequest(next);
      }),
    dialog: request && (
      <Modal
        title={request.title}
        closeLabel={t("close")}
        onClose={() => finish(false)}
      >
        <div className="bc-dialog-body">
          <p className="bc-confirm-message">{request.message}</p>
        </div>
        <footer>
          <button onClick={() => finish(false)}>{t("cancel")}</button>
          <button
            className={request.danger ? "danger" : "primary"}
            onClick={() => finish(true)}
          >
            {request.action}
          </button>
        </footer>
      </Modal>
    ),
  };
}
