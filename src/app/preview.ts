/** Deterministic, memory-only fixture for browser visual QA; never used in native builds. */
import type {
  BookChapterView,
  JobView,
  GlossaryTermView,
  ModelView,
} from "../shared/contracts/generated";
const project = {
  id: "preview-book",
  kind: "book",
  name: "Сад за морем",
  formatVersion: 1,
  createdAt: "2026-09-24T00:00:00Z",
  source: {
    format: "epub",
    displayName: "The Garden Beyond the Sea.epub",
    originalPath: null,
  },
};
const manga = {
  ...project,
  id: "preview-manga",
  kind: "manga",
  name: "Манга · 1000 страниц",
  source: {
    format: "directory",
    displayName: "Preview pages",
    originalPath: null,
  },
};
const mangaPages = Array.from({ length: 1000 }, (_, i) => ({
  id: `page-${i}`,
  volumeId: `volume-${Math.floor(i / 100)}`,
  position: i % 100,
  originalAssetId: "preview-page",
  thumbnailAssetId: "preview-page",
  width: 640,
  height: 900,
  revision: "0",
}));
const chapters = [
  "The last ferry",
  "A garden in the rain",
  "Letters from the coast",
].map((title, position) => ({
  id: `chapter-${position}`,
  position,
  title,
  revision: "0",
}));
const original = [
  "The last ferry left the harbour just before sunset. On the quay, Anna stood with a small suitcase and a letter she had read so often that the paper had softened along the folds.",
  "Beyond the water, the island was little more than a dark line. She could make out the white tower above the trees, and, below it, the garden her grandfather had described.",
  "“There is always something flowering,” he had written. “Even when it seems that everything else has stopped.”",
];
const translated = [
  "Последний паром ушёл из гавани перед самым закатом. На причале стояла Анна с небольшим чемоданом и письмом, которое она перечитывала так часто, что бумага на сгибах стала мягкой.",
  "За водой остров казался тонкой тёмной полосой. Над деревьями виднелась белая башня, а под ней — сад, о котором рассказывал дедушка.",
  "«Здесь всегда что-нибудь цветёт, — писал он. — Даже когда кажется, что всё остальное замерло».",
];
const views: BookChapterView[] = chapters.map((chapter) => ({
  chapter,
  instructions: "",
  translation: {
    id: `translation-${chapter.id}`,
    revision: "1",
    title: ["Последний паром", "Сад под дождём", "Письма с побережья"][
      chapter.position
    ],
    status: "ready",
  },
  blocks: original.map((text, position) => ({
    id: `${chapter.id}-block-${position}`,
    chapterId: chapter.id,
    position,
    revision: "0",
    content: { kind: "text", text },
    translatedText: translated[position],
  })),
}));
let glossary: GlossaryTermView[] = [
  {
    id: "anna",
    source: "Anna",
    target: "Анна",
    kind: "character",
    pinned: true,
    frequency: 12,
    revision: "1",
  },
  {
    id: "island",
    source: "the island",
    target: "остров",
    kind: "place",
    pinned: false,
    frequency: 8,
    revision: "1",
  },
];
let settingsRevision = 1;
const jobs: JobView[] = [];
const previewModel: ModelView = {
  model: {
    id: "preview-lama",
    name: "LaMa ONNX · FP32",
    repository: "Carve/LaMa-ONNX",
    revision: "preview",
    filename: "lama_fp32.onnx",
    sha256: "preview",
    bytes: 208044816,
    license: "Apache-2.0",
    experimental: true,
  },
  status: "missing",
  downloadedBytes: 0,
  failure: null,
};
export async function invokePreview<T>(
  command: string,
  raw?: Record<string, unknown>,
): Promise<T> {
  const args = (raw?.args ?? {}) as Record<string, any>;
  let result: unknown;
  switch (command) {
    case "model_list":
      if (previewModel.status === "downloading") {
        previewModel.downloadedBytes = Math.min(
          previewModel.model.bytes,
          previewModel.downloadedBytes + 10402240,
        );
        if (previewModel.downloadedBytes === previewModel.model.bytes)
          previewModel.status = "verifying";
      } else if (previewModel.status === "verifying")
        previewModel.status = "downloaded";
      result = [previewModel];
      break;
    case "model_download":
      previewModel.status = "downloading";
      break;
    case "model_pause":
      previewModel.status = "paused";
      break;
    case "model_remove":
      previewModel.status = "missing";
      previewModel.downloadedBytes = 0;
      break;
    case "project_list":
      result = [
        {
          descriptor: project,
          progress: { kind: "book", chapters: 3, translated: 3 },
        },
        {
          descriptor: manga,
          progress: { kind: "manga", pages: 1000, approved: 0, lettered: 0 },
        },
      ];
      break;
    case "project_open":
      result = args.projectId === manga.id ? manga : project;
      break;
    case "provider_profiles_list":
      result = previewProfiles;
      break;
    case "provider_profile_save": {
      const saved = {
        ...args.profile,
        hasKey: !!args.credential || args.profile.hasKey,
        revision: String(Number(args.profile.revision) + 1),
      };
      previewProfiles = previewProfiles
        .filter((p) => p.id !== saved.id)
        .concat(saved);
      result = saved;
      break;
    }
    case "project_settings_update":
      previewRoles = args.choices;
      result = String(++settingsRevision);
      break;
    case "project_settings_get":
      result = {
        languages: { source: "en", target: "ru" },
        choices: previewRoles,
        revision: "1",
      };
      break;
    case "manga_list_pages": {
      const start = args.cursor ? Number(args.cursor) : 0;
      result = {
        items: mangaPages.slice(start, start + args.limit),
        nextCursor:
          start + args.limit < mangaPages.length
            ? String(start + args.limit)
            : null,
      };
      break;
    }
    case "book_list_chapters":
      result = { items: chapters, nextCursor: null };
      break;
    case "book_search": {
      const normalize = (s: string) =>
        args.caseSensitive ? s : s.toLocaleLowerCase();
      const matches = views.flatMap((v) =>
        v.blocks
          .filter((b) =>
            normalize(
              args.side === "source"
                ? b.content.kind !== "image"
                  ? b.content.text
                  : ""
                : (b.translatedText ?? ""),
            ).includes(normalize(args.query)),
          )
          .map((b) => ({
            chapterId: v.chapter.id,
            blockId: b.id,
            title: v.chapter.title,
            snippet:
              args.side === "source"
                ? b.content.kind !== "image"
                  ? b.content.text
                  : ""
                : (b.translatedText ?? ""),
          })),
      );
      result = { matches, nextCursor: null };
      break;
    }
    case "book_get_chapter":
      result = views.find((v) => v.chapter.id === args.chapterId);
      break;
    case "book_update_translation_block": {
      const view = views.find((v) => v.translation?.id === args.translationId)!;
      if (
        !view.translation ||
        view.translation.revision !== args.expectedRevision
      )
        throw { code: "revision_conflict" };
      const revision = String(Number(view.translation.revision) + 1);
      view.blocks.find((b) => b.id === args.blockId)!.translatedText =
        args.text;
      view.translation = {
        ...view.translation,
        revision,
        id: `${view.chapter.id}-translation-${revision}`,
      };
      result = revision;
      break;
    }
    case "book_update_instructions": {
      const view = views.find((v) => v.chapter.id === args.chapterId)!;
      view.instructions = args.instructions;
      view.chapter.revision = String(Number(view.chapter.revision) + 1);
      result = view.chapter.revision;
      break;
    }
    case "assistant_project_view":
      result = previewAssistant;
      break;
    case "assistant_project_send":
      previewAssistant.messages.push(
        {
          id: `user-${previewAssistant.messages.length}`,
          role: "user",
          text: args.message,
        },
        {
          id: `assistant-${previewAssistant.messages.length}`,
          role: "assistant",
          text: "Предлагаю уточнить общий промпт книги. Проверьте изменение перед применением.",
        },
      );
      previewAssistant.proposals.push({
        id: `proposal-${previewAssistant.messages.length}`,
        kind: "book_prompt",
        before: previewPresentation.instructions,
        after: "Сохраняй имена персонажей и единый стиль повествования.",
      });
      result = { ...previewAssistant };
      break;
    case "assistant_project_confirm": {
      const proposal = previewAssistant.proposals.find(
        (p) => p.id === args.proposalId,
      );
      if (args.approved && proposal) {
        previewPresentation.instructions = proposal.after;
        previewPresentation.revision = String(
          Number(previewPresentation.revision) + 1,
        );
      }
      previewAssistant.proposals = previewAssistant.proposals.filter(
        (p) => p.id !== args.proposalId,
      );
      result = null;
      break;
    }
    case "assistant_project_cancel":
      result = null;
      break;
    case "book_presentation_get":
      result = previewPresentation;
      break;
    case "book_presentation_update":
      if (args.expectedRevision !== previewPresentation.revision)
        throw { code: "revision_conflict" };
      previewPresentation = {
        ...previewPresentation,
        title: args.title,
        author: args.author,
        summary: args.summary,
        instructions: args.instructions,
        revision: String(Number(previewPresentation.revision) + 1),
      };
      result = previewPresentation;
      break;
    case "book_metadata_get":
      result = {
        title: "Сад за морем",
        author: "",
        summary:
          "Вернувшись на остров после долгого отсутствия, Анна находит письма, которые меняют её представление о семье и старом саде у моря.",
        current: true,
      };
      break;
    case "book_reference_get":
      result = { chapters: [], mappings: [], fingerprint: "preview" };
      break;
    case "glossary_list":
      result = {
        items: glossary,
        nextCursor: null,
        settingsRevision: String(settingsRevision),
      };
      break;
    case "glossary_put": {
      const old = glossary.find((v) => v.id === args.termId);
      if (
        String(settingsRevision) !== args.expectedSettingsRevision ||
        (old && old.revision !== args.expectedRevision)
      )
        throw { code: "revision_conflict" };
      glossary = glossary.filter((v) => v.id !== args.termId);
      glossary.push({
        id: args.termId,
        source: args.source,
        target: args.target,
        kind: args.kind,
        pinned: args.pinned,
        frequency: old?.frequency ?? 0,
        revision: String(Number(old?.revision ?? 0) + 1),
      });
      settingsRevision++;
      result = String(settingsRevision);
      break;
    }
    case "job_list":
      result = jobs;
      break;
    case "job_get":
      result = jobs.find((v) => v.job.jobId === args.jobId);
      break;
    case "book_start_translation":
    case "book_start_metadata":
    case "book_start_glossary": {
      const job = { projectId: project.id, jobId: `job-${jobs.length}` };
      jobs.push({
        job,
        kind: command.replace("book_start_", "book_"),
        state: "succeeded",
        revision: "1",
        totalSteps: 3,
        completedSteps: 3,
        error: null,
      });
      result = job;
      break;
    }
    case "get_effective_config":
      result = {
        model: "preview-model",
        base_url: "https://example.invalid",
        has_key: false,
        env_locked: [],
      };
      break;
    case "set_setting":
    case "set_api_key":
      break;
    default:
      throw new Error(`Preview does not implement ${command}`);
  }
  return structuredClone(result) as T;
}

let previewPresentation: import("../shared/contracts/generated").BookPresentation =
  {
    title: null,
    author: null,
    summary: null,
    instructions: "",
    coverAssetId: null,
    revision: "0",
  };

const previewAssistant: import("../shared/contracts/generated").AssistantView =
  { messages: [], proposals: [] };

let previewProfiles: import("../shared/contracts/generated").ProviderEntry[] =
  [];
let previewRoles: import("../shared/contracts/generated").ProcessingChoices = {
  bookTranslationProfile: null,
  mangaRecognitionProfile: null,
  mangaTranslationProfile: null,
  assistantProfile: null,
};
