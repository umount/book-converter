import { useEffect, useRef, type ReactNode } from "react";
export function closeMenus() {
  document
    .querySelectorAll<HTMLDetailsElement>(
      ".bc-menu[open], .bc-export-menu[open]",
    )
    .forEach((menu) => {
      menu.open = false;
    });
}
export function Menu({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    const outside = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node) && ref.current)
        ref.current.open = false;
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, []);
  return (
    <details
      ref={ref}
      className="bc-menu"
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null))
          e.currentTarget.open = false;
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.currentTarget.open = false;
          e.currentTarget.querySelector("summary")?.focus();
        }
      }}
    >
      <summary>{label}</summary>
      <div className="bc-menu-popup">{children}</div>
    </details>
  );
}
