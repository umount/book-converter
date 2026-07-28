import { useEffect, useState } from "react";
import type { CallFn } from "../api";
import type { AppInfo } from "../types";
import { AppIcon } from "./common/AppIcon";
import { Modal } from "./common/Modal";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  call: CallFn;
  onClose: () => void;
};

/** About dialog: product name, version and the build it came from. */
export function About({ t, call, onClose }: Props) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    void (async () => setInfo((await call<AppInfo>("get_app_info")) ?? null))();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The webview version is only known to the webview itself, not to the backend.
  const engine = navigator.userAgent.match(/(WebKit|Chrome|Gecko)\/([\d.]+)/i);
  const rows: [string, string][] = info
    ? [
        [t("about.version"), info.version],
        [t("about.commit"), info.commit],
        [t("about.date"), info.commit_date],
        [t("about.tauri"), info.tauri],
        [t("about.webview"), engine ? `${engine[1]} ${engine[2]}` : "-"],
        [t("about.os"), `${info.os} ${info.arch}`],
      ]
    : [];

  async function copyAll() {
    if (!info) return;
    await navigator.clipboard.writeText(
      [`${info.name} ${info.version}`, ...rows.slice(1).map(([k, v]) => `${k}: ${v}`)].join("\n"),
    );
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  return (
    <Modal className="about" onClose={onClose}>
    <AppIcon className="about-logo" />
    <div className="about-name">{info?.name ?? t("about.title")}</div>
    <div className="about-tagline">{t("about.tagline")}</div>

    <dl className="about-rows">
      {rows.map(([k, v]) => (
        <div className="about-row" key={k}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
      {!info && <div className="muted">{t("reader.loading")}</div>}
    </dl>

    <div className="about-actions">
      <button className="ghost" disabled={!info} onClick={() => void copyAll()}>
        {copied ? t("about.copied") : t("about.copy")}
      </button>
      <button onClick={onClose}>{t("about.close")}</button>
    </div>
    </Modal>
  );
}
