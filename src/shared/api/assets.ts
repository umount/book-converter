import { convertFileSrc } from "@tauri-apps/api/core";
export const assetUrl = (projectId: string, id: string) =>
  import.meta.env.DEV && projectId === "preview-manga"
    ? "/src/app/preview-page.svg"
    : convertFileSrc(`${projectId}/${id}`, "bookasset");
