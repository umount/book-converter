/** Keep pointer-triggered focus restoration from drawing a keyboard focus ring. */
export function installFocusIndicators(doc: Document = document): () => void {
  const root = doc.documentElement;
  const pointer = () => { root.dataset.focusInput = "pointer"; };
  const keyboard = (event: KeyboardEvent) => {
    if (!["Shift", "Control", "Alt", "Meta"].includes(event.key)) {
      root.dataset.focusInput = "keyboard";
    }
  };
  doc.addEventListener("pointerdown", pointer, true);
  doc.addEventListener("keydown", keyboard, true);
  return () => {
    doc.removeEventListener("pointerdown", pointer, true);
    doc.removeEventListener("keydown", keyboard, true);
    delete root.dataset.focusInput;
  };
}
