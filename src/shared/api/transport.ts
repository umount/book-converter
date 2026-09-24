import type { ProjectEvent } from "../contracts/generated";

export interface Transport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, receive: (payload: T) => void): Promise<() => void>;
}

/** Preserve structured backend errors; callers decide how to localize them. */
export function createProjectApi(transport: Transport) {
  return {
    inspectManifest(path: string) {
      return transport.invoke<import("../contracts/generated").ProjectDescriptor>(
        "project_inspect_manifest", { path },
      );
    },
    subscribe(projectId: string, receive: (event: ProjectEvent) => void) {
      let disposed = false;
      let unlisten: (() => void) | undefined;
      const ready = transport.listen<ProjectEvent>("project-event", (event) => {
        if (!disposed && event.version === 1 && event.projectId === projectId) receive(event);
      }).then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      });
      return {
        ready,
        dispose() {
          disposed = true;
          unlisten?.();
          unlisten = undefined;
        },
      };
    },
  };
}
