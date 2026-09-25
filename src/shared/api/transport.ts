import type * as C from "../contracts/generated";
import type { ProjectEvent } from "../contracts/generated";

export interface Transport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, receive: (payload: T) => void): Promise<() => void>;
}

/** Preserve structured backend errors; callers decide how to localize them. */
export function createProjectApi(transport: Transport) {
  const call = <T>(command: string, args: unknown) =>
    transport.invoke<T>(command, { args });
  return {
    profiles: () =>
      transport.invoke<C.ProviderEntry[]>("provider_profiles_list"),
    saveProfile: (args: C.SaveProviderArgs) =>
      call<C.ProviderEntry>("provider_profile_save", args),
    assistantView: (args: C.ProjectArgs) =>
      call<C.AssistantView>("assistant_project_view", args),
    assistantSend: (args: C.AssistantSendArgs) =>
      call<C.AssistantView>("assistant_project_send", args),
    assistantConfirm: (args: C.AssistantConfirmArgs) =>
      call<C.JobRef | null>("assistant_project_confirm", args),
    assistantCancel: (args: C.ProjectArgs) =>
      call<void>("assistant_project_cancel", args),
    list: () => transport.invoke<C.ProjectSummary[]>("project_list"),
    inspectSource: (args: C.InspectSourceArgs) =>
      call<C.ImportPreview>("project_inspect_source", args),
    create: (args: C.CreateProjectArgs) =>
      call<C.ProjectDescriptor>("project_create", args),
    cancelImport: (args: C.ImportSessionArgs) =>
      call<void>("project_cancel_import", args),
    open: (args: C.ProjectArgs) =>
      call<C.ProjectDescriptor>("project_open", args),
    delete: (args: C.ProjectArgs) => call<void>("project_delete", args),
    exportArchive: (args: C.ArchiveExportArgs) =>
      call<void>("project_archive_export", args),
    importArchive: (args: C.ArchiveImportArgs) =>
      call<C.ProjectDescriptor>("project_archive_import", args),
    startMangaStage: (args: C.StartMangaStageArgs) =>
      call<C.JobRef>("manga_start_stage", args),
    startMangaAutomatic: (args: C.StartMangaRunArgs) =>
      call<C.JobRef>("manga_start_automatic", args),
    mangaPreflight: (args: C.ProjectArgs) =>
      call<C.MangaPreflight>("manga_preflight", args),
    mangaPage: (args: C.GetMangaPageArgs) =>
      call<C.MangaPageView>("manga_get_page", args),
    mangaVolumes: (args: C.ProjectArgs) =>
      call<C.MangaVolumeSummary[]>("manga_list_volumes", args),
    mangaPages: (args: C.ListMangaPagesArgs) =>
      call<C.PageSummaryPage>("manga_list_pages", args),
    chapters: (args: C.ListChaptersArgs) =>
      call<C.ChapterPage>("book_list_chapters", args),
    chapter: (args: C.GetChapterArgs) =>
      call<C.BookChapterView>("book_get_chapter", args),
    translate: (args: C.StartBookTranslationArgs) =>
      call<C.JobRef>("book_start_translation", args),
    searchBook: (args: C.BookSearchArgs) =>
      call<C.BookSearchPage>("book_search", args),
    previewReplace: (args: C.BookReplacePreviewArgs) =>
      call<C.BookReplacePreview>("book_replace_preview", args),
    applyReplace: (args: C.BookReplaceApplyArgs) =>
      call<number>("book_replace_apply", args),
    settings: (args: C.ProjectArgs) =>
      call<C.ProjectSettingsView>("project_settings_get", args),
    updateSettings: (args: C.ProjectSettingsUpdateArgs) =>
      call<C.Revision>("project_settings_update", args),
    glossary: (args: C.GlossaryListArgs) =>
      call<C.GlossaryPage>("glossary_list", args),
    putTerm: (args: C.GlossaryPutArgs) =>
      call<C.Revision>("glossary_put", args),
    deleteTerm: (args: C.GlossaryDeleteArgs) =>
      call<void>("glossary_delete", args),
    extractGlossary: (args: C.StartBookGlossaryArgs) =>
      call<C.JobRef>("book_start_glossary", args),
    startMetadata: (args: C.StartBookMetadataArgs) =>
      call<C.JobRef>("book_start_metadata", args),
    presentation: (args: C.ProjectArgs) =>
      call<C.BookPresentation>("book_presentation_get", args),
    updatePresentation: (args: C.UpdateBookPresentationArgs) =>
      call<C.BookPresentation>("book_presentation_update", args),
    setCover: (args: C.SetBookCoverArgs) =>
      call<C.BookPresentation>("book_cover_set", args),
    metadata: (args: C.ProjectArgs) =>
      call<C.BookMetadataView | null>("book_metadata_get", args),
    importReference: (args: C.BookReferenceImportArgs) =>
      call<C.BookReferenceView>("book_reference_import", args),
    reference: (args: C.ProjectArgs) =>
      call<C.BookReferenceView>("book_reference_get", args),
    mapReference: (args: C.BookReferenceMapArgs) =>
      call<C.BookReferenceView>("book_reference_map", args),
    exportBook: (args: C.BookExportArgs) => call<void>("book_export", args),
    updateInstructions: (args: C.UpdateChapterInstructionsArgs) =>
      call<C.Revision>("book_update_instructions", args),
    editSource: (args: C.UpdateBookBlockArgs) =>
      call<C.Revision>("book_update_block", args),
    editTitle: (args: C.UpdateTranslationTitleArgs) =>
      call<C.Revision>("book_update_translation_title", args),
    retargetPreview: (args: C.StartBookRetargetArgs) =>
      call<C.BookRetargetPreview>("book_retarget_preview", args),
    retarget: (args: C.StartBookRetargetArgs) =>
      call<C.JobRef>("book_start_retarget", args),
    translateTitle: (args: C.StartBookTitleArgs) =>
      call<C.JobRef>("book_start_title", args),
    editTranslation: (args: C.UpdateTranslationBlockArgs) =>
      call<C.Revision>("book_update_translation_block", args),
    jobs: (args: C.ListJobsArgs) => call<C.JobView[]>("job_list", args),
    job: (args: C.JobArgs) => call<C.JobView>("job_get", args),
    cancelJob: (args: C.JobArgs) => call<void>("job_cancel", args),
    resumeJob: (args: C.JobArgs) => call<C.JobRef>("job_resume", args),
    inspectManifest(path: string) {
      return transport.invoke<
        import("../contracts/generated").ProjectDescriptor
      >("project_inspect_manifest", { path });
    },
    subscribe(projectId: string, receive: (event: ProjectEvent) => void) {
      let disposed = false;
      let unlisten: (() => void) | undefined;
      const ready = transport
        .listen<ProjectEvent>("project-event", (event) => {
          if (!disposed && event.version === 1 && event.projectId === projectId)
            receive(event);
        })
        .then((stop) => {
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
