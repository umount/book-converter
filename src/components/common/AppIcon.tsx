type Props = { className?: string };

/** App mark: an open book, matching src-tauri/icons/icon.svg. */
export function AppIcon({ className = "" }: Props) {
  return (
    <svg className={`app-icon ${className}`} viewBox="0 0 512 512" aria-hidden="true">
      <defs>
        <linearGradient id="bcMark" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#4c8dff" />
          <stop offset="1" stopColor="#2b5cc4" />
        </linearGradient>
      </defs>
      <rect width="512" height="512" rx="112" fill="url(#bcMark)" />
      <g fill="none" stroke="#ffffff" strokeWidth="26" strokeLinejoin="round" strokeLinecap="round">
        <path d="M256 152 C 212 122, 152 122, 112 142 L112 372 C152 352, 212 352, 256 380" />
        <path d="M256 152 C 300 122, 360 122, 400 142 L400 372 C360 352, 300 352, 256 380" />
        <line x1="256" y1="152" x2="256" y2="380" />
      </g>
    </svg>
  );
}
