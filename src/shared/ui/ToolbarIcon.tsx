export function ToolbarIcon({
  name,
}: {
  name: "add" | "assistant" | "jobs" | "settings" | "search" | "reference" | "manual" | "model";
}) {
  const paths = {
    reference: "M12 5v15M12 5C9 3 5 3 2 4v15c3-1 7-1 10 1 3-2 7-2 10-1V4c-3-1-7-1-10 1Z",
    manual: "m4 16-1 5 5-1L20 8l-4-4L4 16Zm9-9 4 4",
    model: "m12 2 3 7 7 3-7 3-3 7-3-7-7-3 7-3 3-7Z",

    search: "M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z",
    add: "M12 5v14M5 12h14",
    assistant:
      "M5 4h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9l-6 3V6a2 2 0 0 1 2-2ZM7 9h10M7 13h6",
    jobs: "M4 5h3v3H4zM11 6h9M4 11h3v3H4zM11 12h9M4 17h3v3H4zM11 18h9",
    settings: "M4 7h7m4 0h5M4 17h3m4 0h9M11 4v6M7 14v6",
  };
  return (
    <svg
      aria-hidden="true"
      width="17"
      height="17"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={paths[name]} />
    </svg>
  );
}
