import type * as C from "../contracts/generated";
import type { ProjectEvent } from "../contracts/generated";

export interface Transport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, receive: (payload: T) => void): Promise<() => void>;
}

/** Preserve structured backend errors; callers decide how to localize them. */
export function createProjectApi(transport: Transport) {
  const call = <T>(command: string, args: unknown) => transport.invoke<T>(command, { args });
  return {
    list: () => transport.invoke<C.ProjectSummary[]>("project_list"),
    inspectSource: (args: C.InspectSourceArgs) => call<C.ImportPreview>("project_inspect_source", args),
    create: (args: C.CreateProjectArgs) => call<C.ProjectDescriptor>("project_create", args),
    cancelImport: (args: C.ImportSessionArgs) => call<void>("project_cancel_import", args),
    open: (args: C.ProjectArgs) => call<C.ProjectDescriptor>("project_open", args),
    delete: (args: C.ProjectArgs) => call<void>("project_delete", args),
    exportArchive: (args: C.ArchiveExportArgs) => call<void>("project_archive_export", args),
    importArchive: (args: C.ArchiveImportArgs) => call<C.ProjectDescriptor>("project_archive_import", args),
    chapters: (args: C.ListChaptersArgs) => call<C.ChapterPage>("book_list_chapters", args),
    chapter: (args: C.GetChapterArgs) => call<C.BookChapterView>("book_get_chapter", args),
    translate: (args: C.StartBookTranslationArgs) => call<C.JobRef>("book_start_translation", args),
    previewReplace: (args: C.BookReplacePreviewArgs) => call<C.BookReplacePreview>("book_replace_preview", args),
    applyReplace: (args: C.BookReplaceApplyArgs) => call<number>("book_replace_apply", args),
    editSource: (args: C.UpdateBookBlockArgs) => call<C.Revision>("book_update_block", args),
    editTranslation: (args: C.UpdateTranslationBlockArgs) => call<C.Revision>("book_update_translation_block", args),
    jobs: (args: C.ListJobsArgs) => call<C.JobView[]>("job_list", args),
    job: (args: C.JobArgs) => call<C.JobView>("job_get", args),
    cancelJob: (args: C.JobArgs) => call<void>("job_cancel", args),
    resumeJob: (args: C.JobArgs) => call<C.JobRef>("job_resume", args),
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
