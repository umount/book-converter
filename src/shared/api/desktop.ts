import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
// Fixtures are loaded only by an explicitly requested development browser preview.
export const previewMode = import.meta.env.DEV && !isTauri() && new URLSearchParams(location.search).get("preview") === "1";
export const desktopInvoke: typeof invoke = (command, args, options) => previewMode
  ? import("../../app/preview").then(module => module.invokePreview(command, args as Record<string, unknown>))
  : invoke(command, args, options);
export const desktopListen = <T>(event: string, receive: (payload: T) => void): Promise<() => void> => previewMode
  ? Promise.resolve(() => {}) : listen<T>(event, ({ payload }) => receive(payload));
