import { convertFileSrc } from "@tauri-apps/api/core";
export const assetUrl = (projectId: string, id: string) => convertFileSrc(`${projectId}/${id}`, "bookasset");
