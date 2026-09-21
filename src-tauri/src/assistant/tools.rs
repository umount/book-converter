//! Single allowlist: schema, confirm policy, and UI invalidation.

use serde::Serialize;

use crate::translator::deepseek::ToolFunction;
use crate::translator::ToolSpec;

use super::args::{
    validator, validator_replace_in_book, BootstrapGlossaryArgs, ChapterTermsArgs, DeleteTermArgs,
    EmptyArgs, ExportBookArgs, GetChapterArgs, GlossaryPageArgs, HarvestGlossaryArgs,
    ListChaptersArgs, ReplaceInBookArgs, ResetTranslationArgs, RetargetTermsArgs, SearchBookArgs,
    SetBookPromptArgs, SetChapterContextArgs, SetChapterPromptArgs, StartTranslationArgs, ToolArgs,
    TranslateChapterArgs, UpdateChapterTranslationArgs, UpdateTermArgs,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolPolicy {
    Auto,
    Confirm,
    Heavy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Invalidate {
    Progress,
    Chapters,
    OpenChapter,
    Glossary,
    BookDetails,
    Reference,
}

#[allow(dead_code)]
const _FRONTEND_INVALIDATES: &[Invalidate] = &[Invalidate::Reference];

pub(crate) struct ToolDef {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) policy: ToolPolicy,
    pub(crate) invalidates: &'static [Invalidate],
    pub(crate) untrusted_output: bool,
    pub(crate) schema: &'static str,
    pub(crate) validate: fn(&serde_json::Value) -> Result<(), String>,
}

const PROGRESS_CHAPTERS: &[Invalidate] = &[
    Invalidate::Progress,
    Invalidate::Chapters,
    Invalidate::OpenChapter,
];
const GLOSSARY: &[Invalidate] = &[Invalidate::Glossary];
const GLOSSARY_CHAPTERS: &[Invalidate] = &[
    Invalidate::Glossary,
    Invalidate::Chapters,
    Invalidate::OpenChapter,
];
const CHAPTERS: &[Invalidate] = &[Invalidate::Chapters, Invalidate::OpenChapter];
const OPEN: &[Invalidate] = &[Invalidate::OpenChapter];
const BOOK_DETAILS: &[Invalidate] = &[Invalidate::BookDetails];

pub(crate) const TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "get_progress",
        description: "Current translation progress counts and whether a job is running.",
        policy: ToolPolicy::Auto,
        invalidates: &[],
        untrusted_output: false,
        schema: EmptyArgs::SCHEMA,
        validate: validator::<EmptyArgs>,
    },
    ToolDef {
        name: "list_chapters",
        description: "List chapters (reading-order idx, book number, titles, status, lang_issues).",
        policy: ToolPolicy::Auto,
        invalidates: &[],
        untrusted_output: true,
        schema: ListChaptersArgs::SCHEMA,
        validate: validator::<ListChaptersArgs>,
    },
    ToolDef {
        name: "get_chapter",
        description: "Load one chapter by reading-order idx (source + translation, truncated).",
        policy: ToolPolicy::Auto,
        invalidates: &[],
        untrusted_output: true,
        schema: GetChapterArgs::SCHEMA,
        validate: validator::<GetChapterArgs>,
    },
    ToolDef {
        name: "search_book",
        description: "Search translations (or source) for a query.",
        policy: ToolPolicy::Auto,
        invalidates: &[],
        untrusted_output: true,
        schema: SearchBookArgs::SCHEMA,
        validate: validator::<SearchBookArgs>,
    },
    ToolDef {
        name: "get_glossary_page",
        description: "Paged glossary lookup.",
        policy: ToolPolicy::Auto,
        invalidates: &[],
        untrusted_output: true,
        schema: GlossaryPageArgs::SCHEMA,
        validate: validator::<GlossaryPageArgs>,
    },
    ToolDef {
        name: "chapter_terms",
        description: "Glossary terms that appear in a chapter's source text.",
        policy: ToolPolicy::Auto,
        invalidates: &[],
        untrusted_output: true,
        schema: ChapterTermsArgs::SCHEMA,
        validate: validator::<ChapterTermsArgs>,
    },
    ToolDef {
        name: "get_book_details",
        description: "Title, author, summary metadata.",
        policy: ToolPolicy::Auto,
        invalidates: &[],
        untrusted_output: true,
        schema: EmptyArgs::SCHEMA,
        validate: validator::<EmptyArgs>,
    },
    ToolDef {
        name: "get_reference_info",
        description: "Reference translation import stats.",
        policy: ToolPolicy::Auto,
        invalidates: &[],
        untrusted_output: false,
        schema: EmptyArgs::SCHEMA,
        validate: validator::<EmptyArgs>,
    },
    ToolDef {
        name: "start_translation",
        description: "Start translating pending chapters. Optional limit.",
        policy: ToolPolicy::Confirm,
        invalidates: PROGRESS_CHAPTERS,
        untrusted_output: false,
        schema: StartTranslationArgs::SCHEMA,
        validate: validator::<StartTranslationArgs>,
    },
    ToolDef {
        name: "pause_translation",
        description: "Request pause after the current chapter.",
        policy: ToolPolicy::Confirm,
        invalidates: PROGRESS_CHAPTERS,
        untrusted_output: false,
        schema: EmptyArgs::SCHEMA,
        validate: validator::<EmptyArgs>,
    },
    ToolDef {
        name: "translate_chapter",
        description: "Translate or retranslate a single chapter by reading-order idx.",
        policy: ToolPolicy::Confirm,
        invalidates: PROGRESS_CHAPTERS,
        untrusted_output: false,
        schema: TranslateChapterArgs::SCHEMA,
        validate: validator::<TranslateChapterArgs>,
    },
    ToolDef {
        name: "translate_chapter_title",
        description: "Translate only a chapter's title (not the body). Use to fix title wording without retranslating the chapter.",
        policy: ToolPolicy::Confirm,
        invalidates: CHAPTERS,
        untrusted_output: false,
        schema: TranslateChapterArgs::SCHEMA,
        validate: validator::<TranslateChapterArgs>,
    },
    ToolDef {
        name: "reset_translation",
        description: "Reset chapters to pending from a book chapter number. DESTRUCTIVE.",
        policy: ToolPolicy::Heavy,
        invalidates: PROGRESS_CHAPTERS,
        untrusted_output: false,
        schema: ResetTranslationArgs::SCHEMA,
        validate: validator::<ResetTranslationArgs>,
    },
    ToolDef {
        name: "update_term",
        description: "Create or update a glossary term (pinned).",
        policy: ToolPolicy::Confirm,
        invalidates: GLOSSARY,
        untrusted_output: false,
        schema: UpdateTermArgs::SCHEMA,
        validate: validator::<UpdateTermArgs>,
    },
    ToolDef {
        name: "delete_term",
        description: "Delete a glossary term by source form.",
        policy: ToolPolicy::Confirm,
        invalidates: GLOSSARY,
        untrusted_output: false,
        schema: DeleteTermArgs::SCHEMA,
        validate: validator::<DeleteTermArgs>,
    },
    ToolDef {
        name: "retarget_terms",
        description: "Propagate glossary renames into translated text and rolling context.",
        policy: ToolPolicy::Confirm,
        invalidates: GLOSSARY_CHAPTERS,
        untrusted_output: false,
        schema: RetargetTermsArgs::SCHEMA,
        validate: validator::<RetargetTermsArgs>,
    },
    ToolDef {
        name: "harvest_glossary",
        description: "Extract glossary terms from already-translated chapters.",
        policy: ToolPolicy::Confirm,
        invalidates: GLOSSARY,
        untrusted_output: false,
        schema: HarvestGlossaryArgs::SCHEMA,
        validate: validator::<HarvestGlossaryArgs>,
    },
    ToolDef {
        name: "bootstrap_glossary",
        description: "Bootstrap pinned glossary from reference translation pairs.",
        policy: ToolPolicy::Confirm,
        invalidates: GLOSSARY,
        untrusted_output: false,
        schema: BootstrapGlossaryArgs::SCHEMA,
        validate: validator::<BootstrapGlossaryArgs>,
    },
    ToolDef {
        name: "update_chapter_translation",
        description: "Manually save an edited chapter translation.",
        policy: ToolPolicy::Confirm,
        invalidates: CHAPTERS,
        untrusted_output: false,
        schema: UpdateChapterTranslationArgs::SCHEMA,
        validate: validator::<UpdateChapterTranslationArgs>,
    },
    ToolDef {
        name: "set_chapter_prompt",
        description: "Set per-chapter translation instruction. Prefer set_book_prompt for rules that should apply to every chapter.",
        policy: ToolPolicy::Confirm,
        invalidates: OPEN,
        untrusted_output: false,
        schema: SetChapterPromptArgs::SCHEMA,
        validate: validator::<SetChapterPromptArgs>,
    },
    ToolDef {
        name: "set_book_prompt",
        description: "Set a book-wide translation instruction applied to every chapter (title format, register, recurring choices). Empty string clears it. Does not rewrite already translated chapters. Per-chapter prompts override conflicts.",
        policy: ToolPolicy::Confirm,
        invalidates: BOOK_DETAILS,
        untrusted_output: false,
        schema: SetBookPromptArgs::SCHEMA,
        validate: validator::<SetBookPromptArgs>,
    },
    ToolDef {
        name: "set_chapter_context",
        description: "Set rolling summary + prev_tail used before translating this chapter.",
        policy: ToolPolicy::Confirm,
        invalidates: OPEN,
        untrusted_output: false,
        schema: SetChapterContextArgs::SCHEMA,
        validate: validator::<SetChapterContextArgs>,
    },
    ToolDef {
        name: "replace_in_book",
        description: "Literal/regex replace across all translations.",
        policy: ToolPolicy::Confirm,
        invalidates: CHAPTERS,
        untrusted_output: false,
        schema: ReplaceInBookArgs::SCHEMA,
        validate: validator_replace_in_book,
    },
    ToolDef {
        name: "use_reference_as_base",
        description: "Re-seed still-pending chapters from the already imported reference translation.",
        policy: ToolPolicy::Heavy,
        invalidates: PROGRESS_CHAPTERS,
        untrusted_output: false,
        schema: EmptyArgs::SCHEMA,
        validate: validator::<EmptyArgs>,
    },
    ToolDef {
        name: "export_book",
        description: "Export the translated book into the project export folder. Format: fb2, epub, pdf, txt.",
        policy: ToolPolicy::Heavy,
        invalidates: &[],
        untrusted_output: false,
        schema: ExportBookArgs::SCHEMA,
        validate: validator::<ExportBookArgs>,
    },
];

pub(crate) fn find(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|d| d.name == name)
}

pub(crate) fn specs() -> Vec<ToolSpec> {
    TOOLS
        .iter()
        .map(|d| ToolSpec {
            kind: "function",
            function: ToolFunction {
                name: d.name.into(),
                description: d.description.into(),
                parameters: serde_json::from_str(d.schema)
                    .unwrap_or_else(|e| panic!("{}: bad schema: {e}", d.name)),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::args::{validator, GetChapterArgs, ListChaptersArgs};
    use serde_json::Value;

    #[test]
    fn invalidate_labels() {
        assert_eq!(
            serde_json::to_string(&Invalidate::BookDetails).unwrap(),
            "\"book_details\""
        );
        assert_eq!(
            serde_json::to_string(&Invalidate::Reference).unwrap(),
            "\"reference\""
        );
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = TOOLS.iter().map(|d| d.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
    }

    #[test]
    fn schemas_are_objects() {
        for def in TOOLS {
            let schema: Value = serde_json::from_str(def.schema).expect(def.name);
            assert_eq!(schema["type"], "object", "{}", def.name);
        }
    }

    #[test]
    fn invalidates_match_policy() {
        for def in TOOLS {
            if def.policy == ToolPolicy::Auto {
                assert!(def.invalidates.is_empty(), "{}", def.name);
            } else if def.name != "export_book" {
                assert!(!def.invalidates.is_empty(), "{}", def.name);
            }
        }
    }

    #[test]
    fn forbidden_commands_are_absent() {
        for name in [
            "delete_project",
            "set_api_key",
            "load_source",
            "set_setting",
        ] {
            assert!(find(name).is_none(), "{name}");
        }
    }

    #[test]
    fn tool_schemas_match_their_arg_structs() {
        for def in TOOLS {
            let schema: Value =
                serde_json::from_str(def.schema).unwrap_or_else(|e| panic!("{}: {e}", def.name));
            let sample = super::super::args::sample_object(&schema);
            (def.validate)(&sample).unwrap_or_else(|e| panic!("{}: {e}", def.name));
        }
    }

    #[test]
    fn reject_unknown_fields() {
        let extra = serde_json::json!({ "index": 1, "nope": true });
        assert!(validator::<GetChapterArgs>(&extra).is_err());
    }

    #[test]
    fn list_chapters_defaults() {
        let v = serde_json::json!({});
        validator::<ListChaptersArgs>(&v).unwrap();
        let parsed: ListChaptersArgs = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.limit, 40);
        assert_eq!(parsed.offset, 0);
    }

    #[test]
    fn replace_rejects_empty_find() {
        let empty = serde_json::json!({ "find": "", "replace": "x" });
        assert!(validator_replace_in_book(&empty).is_err());
        let ok = serde_json::json!({ "find": "a", "replace": "" });
        assert!(validator_replace_in_book(&ok).is_ok());
    }
}
