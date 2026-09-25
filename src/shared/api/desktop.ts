import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
// Fixtures are loaded only by an explicitly requested development browser preview.
export const previewMode = import.meta.env.DEV && !isTauri() && new URLSearchParams(location.search).get("preview") === "1";
export const desktopInvoke: typeof invoke = async (command, args, options) => {
  if (previewMode)
    return import("../../app/preview").then(module => module.invokePreview(command, args as Record<string, unknown>));
  const started = performance.now();
  let failed = false;
  try {
    return await invoke(command, args, options);
  } catch (error) {
    failed = true;
    throw error;
  } finally {
    if (command !== "diagnostic_event")
      void invoke("diagnostic_event", {
        command,
        elapsedMs: Math.min(0xffffffff, Math.round(performance.now() - started)),
        failed,
      }).catch(() => {});
  }
};
export const desktopListen = <T>(event: string, receive: (payload: T) => void): Promise<() => void> => previewMode
  ? Promise.resolve(() => {}) : listen<T>(event, ({ payload }) => receive(payload));
