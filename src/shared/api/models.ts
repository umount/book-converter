import { desktopInvoke } from "./desktop";
import type { ModelView } from "../contracts/generated";
export const modelApi = {
  list: () => desktopInvoke<ModelView[]>("model_list"),
  download: (modelId: string) =>
    desktopInvoke<void>("model_download", { args: { modelId } }),
  pause: (modelId: string) =>
    desktopInvoke<void>("model_pause", { args: { modelId } }),
  remove: (modelId: string) =>
    desktopInvoke<void>("model_remove", { args: { modelId } }),
};
