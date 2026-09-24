//! Project identity and manifest validation, independent of the legacy session.
mod archive;
mod import;
pub mod lifecycle;
pub mod reset;

use crate::app::contracts::{
    AppError, ProjectDescriptor, ProjectId, ProjectKind, SourceDescriptor,
};
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub id: ProjectId,
    pub kind: ProjectKind,
    pub name: String,
    pub created_at: String,
    pub source: SourceDescriptor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_id: Option<String>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, AppError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| AppError::invalid("manifest"))?;
        if value.get("format_version").and_then(|v| v.as_u64()) != Some(FORMAT_VERSION as u64) {
            return Err(AppError::unsupported_version());
        }
        let manifest: Self =
            serde_json::from_value(value).map_err(|_| AppError::invalid("manifest"))?;
        manifest.id.validate()?;
        if manifest.name.trim().is_empty()
            || manifest.created_at.trim().is_empty()
            || manifest.source.format.trim().is_empty()
            || manifest.source.display_name.trim().is_empty()
        {
            return Err(AppError::invalid("manifest"));
        }
        Ok(manifest)
    }

    pub fn descriptor(self) -> ProjectDescriptor {
        ProjectDescriptor {
            id: self.id,
            kind: self.kind,
            name: self.name,
            format_version: self.format_version,
            created_at: self.created_at,
            source: self.source,
        }
    }
}

/// Read only identity metadata. Opening a project must never trigger AI processing.
pub fn inspect_manifest(path: &std::path::Path) -> Result<ProjectDescriptor, AppError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| AppError {
        code: crate::app::contracts::ErrorCode::NotFound,
        message_key: "errors.projectNotFound".into(),
        params: Default::default(),
        retryable: false,
    })?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AppError::invalid("manifest"))?;
    if bytes.len() > 1024 * 1024 {
        return Err(AppError::invalid("manifestSize"));
    }
    Manifest::parse(&bytes).map(Manifest::descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::contracts::ErrorCode;

    #[test]
    fn old_manifests_are_rejected_explicitly() {
        assert_eq!(
            Manifest::parse(br#"{"name":"Old book"}"#).unwrap_err().code,
            ErrorCode::UnsupportedVersion
        );
    }

    #[test]
    fn supported_manifest_requires_kind_and_valid_identity() {
        let mut value = serde_json::json!({"format_version": 1, "id": ProjectId::new(),
            "kind":"manga", "name":"Example", "created_at":"2026-09-23T00:00:00Z",
            "source":{"format":"cbz","displayName":"Example.cbz","originalPath":null}});
        let parsed = Manifest::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(parsed.descriptor().kind, ProjectKind::Manga);
        value.as_object_mut().unwrap().remove("kind");
        assert!(Manifest::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        value["format_version"] = 2.into();
        assert_eq!(
            Manifest::parse(&serde_json::to_vec(&value).unwrap())
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedVersion
        );
    }
}

#[cfg(test)]
mod lifecycle_tests;
