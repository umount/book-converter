export function ToolbarIcon({
  name,
}: {
  name: "add" | "assistant" | "jobs" | "settings";
}) {
  const paths = {
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
