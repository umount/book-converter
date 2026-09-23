import { invoke } from "@tauri-apps/api/core";
import type { ProjectDescriptor } from "../contracts/generated";

/** Read a versioned manifest without activating a project or starting AI work. */
export function inspectProjectManifest(path: string): Promise<ProjectDescriptor> {
  return invoke<ProjectDescriptor>("project_inspect_manifest", { path });
}
