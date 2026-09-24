//! Canonical wire types. Generate TypeScript with the export_contracts example.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Book,
    Manga,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectId(String);

impl ProjectId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn validate(&self) -> Result<(), AppError> {
        let parsed = uuid::Uuid::parse_str(&self.0).map_err(|_| AppError::invalid("projectId"))?;
        if parsed.hyphenated().to_string() != self.0 || parsed.is_nil() {
            return Err(AppError::invalid("projectId"));
        }
        Ok(())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

/// Domain identity types prevent mixing page, chapter and job references in services.
macro_rules! entity_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        pub struct $name(pub String);
    };
}
entity_id!(ChapterId);
entity_id!(BlockId);
entity_id!(VolumeId);
entity_id!(PageId);
entity_id!(RegionId);
entity_id!(AssetId);
entity_id!(JobId);
entity_id!(ImportId);
entity_id!(TermId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: ErrorCode,
    pub message_key: String,
    pub params: std::collections::BTreeMap<String, String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    UnsupportedVersion,
    WrongProjectKind,
    RevisionConflict,
    NotFound,
    CapabilityUnavailable,
    Storage,
    JobCancelled,
    Provider,
    InvalidOutput,
}

impl AppError {
    pub fn invalid(field: &str) -> Self {
        Self {
            code: ErrorCode::InvalidInput,
            message_key: "errors.invalidInput".into(),
            params: [("field".into(), field.into())].into(),
            retryable: false,
        }
    }

    pub fn unsupported_version() -> Self {
        Self {
            code: ErrorCode::UnsupportedVersion,
            message_key: "errors.unsupportedProjectVersion".into(),
            params: Default::default(),
            retryable: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDescriptor {
    pub id: ProjectId,
    pub kind: ProjectKind,
    pub name: String,
    pub format_version: u32,
    pub created_at: String,
    pub source: SourceDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceDescriptor {
    pub format: String,
    pub display_name: String,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelling,
    Cancelled,
    Interrupted,
}

/// Decimal revision strings avoid JavaScript integer precision loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Revision(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntitySelection {
    All,
    ExplicitIds { ids: Vec<String> },
    Range { first: String, last: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JobRef {
    pub project_id: ProjectId,
    pub job_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload")]
pub enum EventPayload {
    #[serde(rename = "job.updated")]
    JobUpdated { state: JobState },
    #[serde(rename = "entity.changed")]
    EntityChanged { id: String, revision: Revision },
    #[serde(rename = "glossary.changed")]
    GlossaryChanged { revision: Revision },
    #[serde(rename = "assistant.updated")]
    AssistantUpdated { revision: Revision },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEvent {
    pub version: u32,
    pub project_id: ProjectId,
    pub job_id: Option<String>,
    pub seq: Revision,
    #[serde(flatten)]
    pub event: EventPayload,
}

pub fn typescript() -> String {
    let config = ts_rs::Config::default();
    let declarations = [
        ProjectKind::decl(&config),
        ChapterId::decl(&config),
        BlockId::decl(&config),
        VolumeId::decl(&config),
        PageId::decl(&config),
        RegionId::decl(&config),
        AssetId::decl(&config),
        JobId::decl(&config),
        ImportId::decl(&config),
        TermId::decl(&config),
        ProjectId::decl(&config),
        ErrorCode::decl(&config),
        AppError::decl(&config),
        SourceDescriptor::decl(&config),
        ProjectDescriptor::decl(&config),
        JobState::decl(&config),
        Revision::decl(&config),
        EntitySelection::decl(&config),
        JobRef::decl(&config),
        EventPayload::decl(&config),
        ProjectEvent::decl(&config),
        PixelBounds::decl(&config),
        BookBlockContent::decl(&config),
        BookBlockView::decl(&config),
        MangaStage::decl(&config),
        CapabilityRequirement::decl(&config),
        MangaPreflight::decl(&config),
    ];
    let mut output =
        String::from("// Generated from Rust. Run npm run contracts:generate; do not edit.\n");
    for declaration in declarations {
        output.push_str("export ");
        output.push_str(&declaration);
        output.push('\n');
    }
    output.push_str(&super::requests::typescript());
    output
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PixelBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl PixelBounds {
    pub fn validate(&self, page_width: u32, page_height: u32) -> Result<(), AppError> {
        if ![self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            || self.x < 0.0
            || self.y < 0.0
            || self.width <= 0.0
            || self.height <= 0.0
            || self.x + self.width > f64::from(page_width)
            || self.y + self.height > f64::from(page_height)
        {
            return Err(AppError::invalid("bounds"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BookBlockContent {
    Text { text: String },
    Caption { text: String },
    Image { asset_id: String, alt: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BookBlockView {
    pub id: String,
    pub chapter_id: String,
    pub position: u32,
    pub revision: Revision,
    pub content: BookBlockContent,
    pub translated_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MangaStage {
    Detection,
    Recognition,
    Translation,
    Masks,
    Inpainting,
    Lettering,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRequirement {
    pub stage: MangaStage,
    pub available: bool,
    pub reason_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MangaPreflight {
    pub requirements: Vec<CapabilityRequirement>,
}

impl MangaPreflight {
    pub fn ready(&self) -> bool {
        let stages = [
            MangaStage::Detection,
            MangaStage::Recognition,
            MangaStage::Translation,
            MangaStage::Masks,
            MangaStage::Inpainting,
            MangaStage::Lettering,
        ];
        stages.iter().all(|stage| {
            let matches: Vec<_> = self
                .requirements
                .iter()
                .filter(|r| &r.stage == stage)
                .collect();
            matches.len() == 1 && matches[0].available
        })
    }
}

#[cfg(test)]
mod pipeline_tests {
    use super::*;

    #[test]
    fn incomplete_capabilities_cannot_enable_automatic_processing() {
        assert!(!MangaPreflight {
            requirements: vec![]
        }
        .ready());
        let mut preflight = MangaPreflight {
            requirements: [
                MangaStage::Detection,
                MangaStage::Recognition,
                MangaStage::Translation,
                MangaStage::Masks,
                MangaStage::Inpainting,
                MangaStage::Lettering,
            ]
            .into_iter()
            .map(|stage| CapabilityRequirement {
                stage,
                available: true,
                reason_key: None,
            })
            .collect(),
        };
        assert!(preflight.ready());
        preflight.requirements[4].available = false;
        assert!(!preflight.ready());
    }

    #[test]
    fn geometry_must_fit_canonical_pixels() {
        let mut bounds = PixelBounds {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        assert!(bounds.validate(100, 100).is_ok());
        bounds.x = 90.0;
        assert!(bounds.validate(100, 100).is_err());
        bounds.x = f64::NAN;
        assert!(bounds.validate(100, 100).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_reject_paths_and_noncanonical_values() {
        for value in ["../escape", "", "00000000-0000-0000-0000-000000000000"] {
            let id: ProjectId = serde_json::from_value(serde_json::json!(value)).unwrap();
            assert!(id.validate().is_err());
        }
        assert!(ProjectId::new().validate().is_ok());
    }

    #[test]
    fn unknown_kind_is_not_assumed_to_be_a_book() {
        assert!(serde_json::from_str::<ProjectKind>("\"comic\"").is_err());
        assert_eq!(
            serde_json::to_string(&ProjectKind::Manga).unwrap(),
            "\"manga\""
        );
    }

    #[test]
    fn events_preserve_large_revisions() {
        let value = ProjectEvent {
            version: 1,
            project_id: ProjectId::new(),
            job_id: None,
            seq: Revision("9007199254740993".into()),
            event: EventPayload::JobUpdated {
                state: JobState::Interrupted,
            },
        };
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<ProjectEvent>(&json).unwrap(), value);
        assert!(json.contains("job.updated"));
        let wire: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(wire["type"], "job.updated");
        assert_eq!(wire["payload"]["state"], "interrupted");
        assert!(wire.get("event").is_none());
    }
}
