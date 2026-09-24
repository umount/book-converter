import { useEffect, useRef, type ReactNode } from "react";
/** Native dialog supplies focus trapping, Escape handling and focus restoration. */
export function Modal({
  title,
  closeLabel,
  onClose,
  children,
  busy = false,
}: {
  title: string;
  closeLabel: string;
  onClose: () => void;
  children: ReactNode;
  busy?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = ref.current!;
    element.showModal();
    element.querySelector<HTMLElement>("input:not([disabled]), textarea:not([disabled]), select:not([disabled])")?.focus();
    return () => element.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className="bc-dialog"
      aria-label={title}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <header>
        <h2>{title}</h2>
        <button aria-label={closeLabel} disabled={busy} onClick={onClose}>
          ×
        </button>
      </header>
      {children}
    </dialog>
  );
}
