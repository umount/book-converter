/** Deterministic, memory-only fixture for browser visual QA; never used in native builds. */
import type { AudioEngineView, AudioJobView, BookChapterView, BookReplacePreview, BookReferenceView, JobView, GlossaryTermView, } from "../shared/contracts/generated";
let defaultTargetLanguage = "ru";
let replacePreview: BookReplacePreview | null = null;
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
const chapters = [
    "The last ferry",
    "A garden in the rain",
    "Letters from the coast",
].map((title, position) => ({
    translatedVolume: null as string | null,
    volume: new URLSearchParams(location.search).has("volumes") ? (position < 2 ? "第1集" : "第2部 · 第7集") : null,
    id: `chapter-${position}`,
    status: "done",
    origin: "model",
    needsReview: false,
    position,
    title,
    translatedTitle: ["Последний паром", "Сад под дождём", "Письма с побережья"][position],
    revision: "0",
}));
let previewReference: BookReferenceView = {
    chapters: Array.from({ length: 20000 }, (_, position) => ({
        id: `reference-${position}`,
        position,
        title: `Глава ${position + 1}`,
    })),
    mappings: [],
    fingerprint: "0",
};
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
    status: "done",
    langIssues: [],
    translationError: null,
    chapter,
    instructions: "",
    translation: {
        id: `translation-${chapter.id}`,
        revision: "1",
        title: ["Последний паром", "Сад под дождём", "Письма с побережья"][chapter.position],
        status: "ready",
        origin: "model",
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
const volumeTitles = new Map<string, {
    source: string;
    title: string;
    revision: string;
}>();
let settingsRevision = 1;
const jobs: JobView[] = [];
let audioEngine: AudioEngineView = { state: "unloaded", device: null, error: null };
const audioJobs: AudioJobView[] = [{ id: "preview-audio", projectId: project.id, state: "interrupted", voice: "Ryan", device: "auto", text: "translation", language: "Russian", completedChunks: 18, totalChunks: 42, completedChapters: 1, totalChapters: 3, currentChapter: "Сад под дождём", error: null, createdAt: "1791450000000" }];
export async function invokePreview<T>(command: string, raw?: Record<string, unknown>): Promise<T> {
    const args = (raw?.args ?? {}) as Record<string, any>;
    let result: unknown;
    switch (command) {
        case "audio_setup": return { runtimeReady: true, engine: { ...audioEngine }, downloading: false, files: [{ model: { id: "preview-qwen", name: "Qwen3-TTS 0.6B", repository: "Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice", revision: "preview", filename: "model.safetensors", sha256: "preview", bytes: 2498383610, license: "Apache-2.0", experimental: true }, status: "downloaded", downloadedBytes: 2498383610, failure: null }] } as T;
        case "audio_engine_load": {
            audioEngine = { state: "loading", device: args.device, error: null };
            setTimeout(() => { audioEngine = { state: "ready", device: args.device === "cuda" ? "cuda" : "cpu", error: null }; }, 800);
            return null as T;
        }
        case "audio_engine_unload": audioEngine = { state: "unloaded", device: null, error: null }; return null as T;
        case "audio_list": {
            for (const job of audioJobs.filter(j => j.projectId === args.projectId && j.state === "running")) {
                job.completedChunks = Math.min(job.totalChunks, job.completedChunks + 1);
                job.completedChapters = Math.floor(job.completedChunks / job.totalChunks * job.totalChapters);
                if (job.completedChunks === job.totalChunks) job.state = "succeeded";
            }
            return structuredClone(audioJobs.filter(j => j.projectId === args.projectId)) as T;
        }
        case "audio_models_download": case "audio_models_pause": return null as T;
        case "audio_start": {
            audioEngine = { state: "ready", device: args.device === "cuda" ? "cuda" : "cpu", error: null };
            const job: AudioJobView = { ...audioJobs[0], projectId: args.projectId, id: `preview-audio-${audioJobs.length}`, voice: args.voice, text: args.text, device: args.device, state: "running", completedChunks: 0, totalChunks: 12, completedChapters: 0, totalChapters: 1, currentChapter: "Последний паром", createdAt: String(Date.now()) };
            audioJobs.unshift(job);
            return structuredClone(job) as T;
        }
        case "audio_resume": {
            audioEngine = { state: "ready", device: "cpu", error: null };
            const job = audioJobs.find(j => j.id === args.jobId)!;
            job.state = "running";
            return structuredClone(job) as T;
        }
        case "audio_cancel": {
            const job = audioJobs.find(j => j.projectId === args.projectId && j.id === args.jobId);
            if (job?.state === "running") job.state = "paused";
            return null as T;
        }
        case "book_get_volume": return (volumeTitles.get(args.source) ?? { source: args.source, title: "", revision: "0" }) as T;
        case "book_translate_volume": return (args.source === "第1集" ? "Том 1" : "Часть 2 · Том 7") as T;
        case "book_save_volume": {
            const current = volumeTitles.get(args.source);
            if ((current?.revision ?? "0") !== args.expectedRevision)
                throw { messageKey: "errors.revisionConflict" };
            volumeTitles.set(args.source, { source: args.source, title: args.title, revision: String(Number(args.expectedRevision) + 1) });
            for (const c of chapters)
                if (c.volume === args.source)
                    c.translatedVolume = args.title || null;
            return null as T;
        }
        case "project_list":
            result = [
                {
                    descriptor: project,
                    progress: { kind: "book", chapters: 3, translated: 3 },
                },
            ];
            break;
        case "project_open":
            result = project;
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
        case "book_list_chapters":
            result = { items: views.map((v) => v.chapter), nextCursor: null };
            break;
        case "book_replace_preview": {
            const pattern = new RegExp(args.search.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), args.caseSensitive ? "g" : "gi");
            replacePreview = {
                previewId: "preview-replacement",
                changes: views.flatMap((view) => {
                    if (args.selection.kind === "explicit_ids" &&
                        !args.selection.ids.includes(view.chapter.id))
                        return [];
                    return view.blocks.flatMap((block) => {
                        if (block.translatedText == null)
                            return [];
                        const after = block.translatedText.replace(pattern, () => args.replacement);
                        return after === block.translatedText
                            ? []
                            : [
                                {
                                    chapterId: view.chapter.id,
                                    blockId: block.id,
                                    before: block.translatedText,
                                    after,
                                },
                            ];
                    });
                }),
            };
            result = replacePreview;
            break;
        }
        case "book_replace_apply": {
            if (!replacePreview || args.previewId !== replacePreview.previewId)
                throw new Error("No replacement preview");
            for (const change of replacePreview.changes) {
                const view = views.find((v) => v.chapter.id === change.chapterId)!;
                const block = view.blocks.find((b) => b.id === change.blockId)!;
                if (block.translatedText !== change.before)
                    throw { code: "revision_conflict" };
                block.translatedText = change.after;
                if (view.translation)
                    view.translation.revision = String(Number(view.translation.revision) + 1);
            }
            result = replacePreview.changes.length;
            replacePreview = null;
            break;
        }
        case "book_search": {
            const normalize = (s: string) => args.caseSensitive ? s : s.toLocaleLowerCase();
            const matches = views.flatMap((v) => v.blocks
                .filter((b) => normalize(args.side === "source"
                ? b.content.kind !== "image"
                    ? b.content.text
                    : ""
                : (b.translatedText ?? "")).includes(normalize(args.query)))
                .map((b) => ({
                chapterId: v.chapter.id,
                blockId: b.id,
                title: v.chapter.title,
                snippet: args.side === "source"
                    ? b.content.kind !== "image"
                        ? b.content.text
                        : ""
                    : (b.translatedText ?? ""),
            })));
            result = { matches, nextCursor: null };
            break;
        }
        case "book_delete_chapter": {
            const index = views.findIndex(v => v.chapter.id === args.chapterId);
            const view = views[index];
            if (!view || view.chapter.revision !== args.expectedRevision || (view.translation?.revision ?? null) !== args.expectedTranslationRevision)
                throw { code: "revision_conflict" };
            views.splice(index, 1);
            const summaryIndex = chapters.findIndex(c => c.id === args.chapterId);
            if (summaryIndex >= 0)
                chapters.splice(summaryIndex, 1);
            chapters.forEach((c, i) => { c.position = i; });
            views.forEach((v, i) => { v.chapter.position = i; });
            previewReference.mappings = previewReference.mappings.filter(m => m.chapterId !== args.chapterId);
            result = null;
            break;
        }
        case "book_get_chapter":
            result = views.find((v) => v.chapter.id === args.chapterId);
            break;
        case "book_update_translation_title": {
            const view = views.find((v) => v.translation?.id === args.translationId);
            if (!view?.translation ||
                view.translation.revision !== args.expectedRevision)
                throw { code: "revision_conflict" };
            const revision = String(Number(view.translation.revision) + 1);
            view.translation = {
                ...view.translation,
                title: args.title,
                revision,
                id: `${view.chapter.id}-translation-${revision}`,
            };
            result = revision;
            break;
        }
        case "book_update_translation_block": {
            const view = views.find((v) => v.translation?.id === args.translationId)!;
            if (!view.translation ||
                view.translation.revision !== args.expectedRevision)
                throw { code: "revision_conflict" };
            const revision = String(Number(view.translation.revision) + 1);
            view.blocks.find((b) => b.id === args.blockId)!.translatedText =
                args.text;
            view.chapter = { ...view.chapter, origin: "manual" };
            view.translation = {
                ...view.translation,
                origin: "manual",
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
            previewAssistant.messages.push({
                id: `user-${previewAssistant.messages.length}`,
                role: "user",
                text: args.message,
            }, {
                id: `assistant-${previewAssistant.messages.length}`,
                role: "assistant",
                text: "Предлагаю уточнить общий промпт книги. Проверьте изменение перед применением.",
            });
            previewAssistant.proposals.push({
                id: `proposal-${previewAssistant.messages.length}`,
                kind: "book_prompt",
                before: previewPresentation.instructions,
                after: "Сохраняй имена персонажей и единый стиль повествования.",
            });
            result = { ...previewAssistant };
            break;
        case "assistant_project_confirm": {
            const proposal = previewAssistant.proposals.find((p) => p.id === args.proposalId);
            if (args.approved && proposal) {
                previewPresentation.instructions = proposal.after;
                previewPresentation.revision = String(Number(previewPresentation.revision) + 1);
            }
            previewAssistant.proposals = previewAssistant.proposals.filter((p) => p.id !== args.proposalId);
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
                summary: "Вернувшись на остров после долгого отсутствия, Анна находит письма, которые меняют её представление о семье и старом саде у моря.",
                current: true,
            };
            break;
        case "book_reference_get":
            result = previewReference;
            break;
        case "book_reference_map":
            if (args.expectedFingerprint !== previewReference.fingerprint)
                throw { code: "revision_conflict" };
            previewReference = {
                ...previewReference,
                mappings: args.mappings,
                fingerprint: String(Number(previewReference.fingerprint) + 1),
            };
            result = previewReference;
            break;
        case "glossary_list": {
            const matches = glossary.filter((v) => (!args.pinnedOnly || v.pinned) &&
                (v.source.includes(args.query) || v.target.includes(args.query)));
            result = {
                items: matches,
                total: matches.length,
                revision: "0",
                nextCursor: null,
                settingsRevision: String(settingsRevision),
            };
            break;
        }
        case "glossary_put": {
            const old = glossary.find((v) => v.id === args.termId);
            if (String(settingsRevision) !== args.expectedSettingsRevision ||
                (old && old.revision !== args.expectedRevision))
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
        case "book_retarget_preview": {
            const selected = views
                .filter((v) => v.blocks.some((b) => b.translatedText?.includes(args.oldTarget)) ||
                v.translation?.title.includes(args.oldTarget))
                .slice(0, args.maxChapters);
            result = {
                chapters: selected.length,
                fragments: selected.reduce((sum, v) => sum +
                    v.blocks.filter((b) => b.translatedText?.includes(args.oldTarget))
                        .length, 0),
            };
            break;
        }
        case "book_start_retarget":
        case "book_start_title":
        case "book_start_translation":
        case "book_start_metadata":
        case "book_start_glossary": {
            const job = { projectId: project.id, jobId: `job-${jobs.length}` };
            const selected = views
                .filter((v) => args.chapterId
                ? v.chapter.id === args.chapterId
                : args.selection?.kind === "explicit_ids"
                    ? args.selection.ids.includes(v.chapter.id)
                    : true)
                .slice(0, args.maxChapters ??
                (command === "book_start_metadata" ? 1 : views.length));
            const last = selected[selected.length - 1];
            const steps = selected.length *
                (command === "book_start_translation"
                    ? 3
                    : 1);
            jobs.push({
                editingLockedChapters: [],
                job,
                kind: command.replace("book_start_", "book_"),
                state: "succeeded",
                revision: "1",
                totalSteps: steps,
                completedSteps: steps,
                totalChapters: selected.length,
                completedChapters: selected.length,
                currentChapterNumber: last ? last.chapter.position + 1 : null,
                currentChapterTitle: last?.chapter.title ?? null,
                currentStage: command === "book_start_translation"
                    ? "context"
                    : command.replace("book_start_", ""),
                remainingSeconds: null,
                error: null,
            });
            result = job;
            break;
        }
        case "export_diagnostics":
        case "diagnostic_event":
            result = null;
            break;
        case "get_effective_config":
            result = {
                target_lang: defaultTargetLanguage,
                model: "preview-model",
                context_window_tokens: 32768,
                max_output_tokens: 4096,
                base_url: "https://example.invalid",
                has_key: false,
                env_locked: [],
                full_logging: false,
                log_directory: "Preview / logs",
            };
            break;
        case "set_setting":
            if (raw?.key === "target_lang")
                defaultTargetLanguage = String(raw.value);
            break;
        case "set_api_key":
            break;
        default:
            throw new Error(`Preview does not implement ${command}`);
    }
    return structuredClone(result) as T;
}
let previewPresentation: import("../shared/contracts/generated").BookPresentation = {
    sourceTitle: "The Garden Beyond the Sea",
    sourceAuthor: "",
    sourceSummary: null,
    title: null,
    author: null,
    summary: null,
    instructions: "",
    coverAssetId: null,
    revision: "0",
};
const previewAssistant: import("../shared/contracts/generated").AssistantView = { messages: [], proposals: [] };
let previewProfiles: import("../shared/contracts/generated").ProviderEntry[] = [];
let previewRoles: import("../shared/contracts/generated").ProcessingChoices = {
    bookTranslationProfile: null,
    assistantProfile: null,
};
