import { useEffect, type ReactNode } from "react";

type Props = {
  /** Class for the dialog box itself (the backdrop is shared). */
  className?: string;
  onClose: () => void;
  /** When false, only Escape and the dialog's own controls dismiss it. */
  closeOnBackdrop?: boolean;
  labelledBy?: string;
  children: ReactNode;
};

/**
 * Centred dialog over a click-to-dismiss backdrop, closed with Escape.
 *
 * Centring is done with flexbox rather than `translate(-50%, -50%)`: a
 * half-pixel offset from an odd height makes WebKit render the box blurry.
 */
export function Modal({ className = "", onClose, closeOnBackdrop = true, labelledBy, children }: Props) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-backdrop" onClick={closeOnBackdrop ? onClose : undefined}>
      <div
        className={className}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
