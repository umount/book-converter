use crate::app::contracts::{AppError, EntitySelection, ProjectId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioText {
    Original,
    Translation,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioDevice {
    Auto,
    Cpu,
    Cuda,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioState {
    Running,
    Paused,
    Interrupted,
    Failed,
    Succeeded,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioStartArgs {
    pub project_id: ProjectId,
    pub selection: EntitySelection,
    pub text: AudioText,
    pub voice: String,
    pub device: AudioDevice,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioJobArgs {
    pub project_id: ProjectId,
    pub job_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioExportArgs {
    pub project_id: ProjectId,
    pub job_id: String,
    pub destination: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioJobView {
    pub id: String,
    pub project_id: ProjectId,
    pub state: AudioState,
    pub voice: String,
    pub device: AudioDevice,
    pub text: AudioText,
    pub language: String,
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub completed_chapters: u32,
    pub total_chapters: u32,
    pub current_chapter: String,
    pub error: Option<AppError>,
    pub created_at: String,
}
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioSetupView {
    pub runtime_ready: bool,
    pub downloading: bool,
    pub files: Vec<crate::models::ModelView>,
}
pub fn typescript() -> String {
    let c = ts_rs::Config::default();
    [
        AudioText::decl(&c),
        AudioDevice::decl(&c),
        AudioState::decl(&c),
        AudioStartArgs::decl(&c),
        AudioJobArgs::decl(&c),
        AudioExportArgs::decl(&c),
        AudioJobView::decl(&c),
        AudioSetupView::decl(&c),
    ]
    .into_iter()
    .map(|d| format!("export {d}\n"))
    .collect()
}
