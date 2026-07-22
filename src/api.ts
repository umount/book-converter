import { invoke } from "@tauri-apps/api/core";

export type CallFn = <T>(
  name: string,
  args?: Record<string, unknown>,
  opts?: { critical?: boolean },
) => Promise<T | undefined>;

/** Thin invoke wrapper: logs errors and optionally raises the critical banner. */
export function makeCall(
  onError: (msg: string, critical?: boolean) => void,
): CallFn {
  return async <T>(
    name: string,
    args?: Record<string, unknown>,
    opts?: { critical?: boolean },
  ): Promise<T | undefined> => {
    try {
      return await invoke<T>(name, args);
    } catch (e) {
      const msg = String(e);
      onError(msg, opts?.critical);
      return undefined;
    }
  };
}
