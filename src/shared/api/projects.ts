import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createProjectApi } from "./transport";

export const projectApi = createProjectApi({
  invoke,
  listen: <T>(event: string, receive: (payload: T) => void) =>
    listen<T>(event, ({ payload }) => receive(payload)),
});

/** Read a versioned manifest without activating a project or starting AI work. */
export const inspectProjectManifest = projectApi.inspectManifest;
