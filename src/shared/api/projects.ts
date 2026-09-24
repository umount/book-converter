import { desktopInvoke, desktopListen } from "./desktop";
import { createProjectApi } from "./transport";
export const projectApi = createProjectApi({ invoke: desktopInvoke, listen: desktopListen });

/** Read a versioned manifest without activating a project or starting AI work. */
export const inspectProjectManifest = projectApi.inspectManifest;
