//! Local provider profiles. Listing never returns credentials; writes are atomic and revisioned.
use crate::{
    ai::{ChatCompletions, ProviderProfile},
    app::{
        contracts::{AppError, Revision},
        requests::{ProviderEntry, SaveProviderArgs},
    },
    storage::repository::{conflict, storage_error},
};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;
fn get(db: &Connection, key: &str) -> Result<Option<String>, AppError> {
    db.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
        r.get(0)
    })
    .optional()
    .map_err(storage_error)
}
fn put(db: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    db.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[key,value]).map_err(storage_error)?;
    Ok(())
}
fn read(db: &Connection, id: &str) -> Result<Option<ProviderEntry>, AppError> {
    let Some(json) = get(db, &format!("ai_profile:{id}"))? else {
        return Ok(None);
    };
    let p: ProviderProfile =
        serde_json::from_str(&json).map_err(|_| AppError::invalid("providerProfile"))?;
    if p.id != id {
        return Err(AppError::invalid("providerProfile"));
    }
    let revision =
        Revision(get(db, &format!("ai_profile_revision:{id}"))?.unwrap_or_else(|| "0".into()));
    revision.value()?;
    Ok(Some(ProviderEntry {
        id: id.into(),
        name: get(db, &format!("ai_profile_name:{id}"))?.unwrap_or_else(|| id.into()),
        base_url: p.base_url,
        model: p.model,
        temperature: p.temperature,
        max_output_tokens: p.max_output_tokens,
        timeout_seconds: u32::try_from(p.timeout_seconds)
            .map_err(|_| AppError::invalid("providerProfile"))?,
        network_retries: p.network_retries,
        has_key: get(db, &format!("ai_credential:{id}"))?.is_some_and(|s| !s.trim().is_empty()),
        revision,
    }))
}
pub fn list(path: &Path) -> Result<Vec<ProviderEntry>, AppError> {
    let mut db = crate::settings::open(path).map_err(|_| AppError::invalid("settings"))?;
    let tx = db.transaction().map_err(storage_error)?;
    let mut q = tx
        .prepare(
            "SELECT substr(key,12) FROM settings WHERE substr(key,1,11)='ai_profile:' ORDER BY key",
        )
        .map_err(storage_error)?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    ids.into_iter()
        .map(|id| read(&tx, &id)?.ok_or_else(|| AppError::invalid("providerProfile")))
        .collect()
}
pub fn save(path: &Path, args: SaveProviderArgs) -> Result<ProviderEntry, AppError> {
    let p = args.profile;
    if p.id.is_empty()
        || p.id.len() > 64
        || !p
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || p.name.trim().is_empty()
        || p.name.len() > 256
        || p.base_url.len() > 4096
        || p.model.len() > 256
        || args.credential.as_ref().is_some_and(|s| s.len() > 16384)
    {
        return Err(AppError::invalid("providerProfile"));
    }
    let profile = ProviderProfile {
        id: p.id.clone(),
        base_url: p.base_url,
        model: p.model,
        temperature: p.temperature,
        max_output_tokens: p.max_output_tokens,
        timeout_seconds: u64::from(p.timeout_seconds),
        network_retries: p.network_retries,
    };
    // Construction validates transport/options without making a network request.
    ChatCompletions::new(profile.clone(), "validation-only".into())?;
    let mut db = crate::settings::open(path).map_err(|_| AppError::invalid("settings"))?;
    let tx = db.transaction().map_err(storage_error)?;
    let old = read(&tx, &p.id)?;
    if old.as_ref().map(|v| &v.revision) != args.expected_revision.as_ref() {
        return Err(conflict());
    }
    let revision = old
        .as_ref()
        .map(|v| v.revision.value())
        .transpose()?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| AppError::invalid("revision"))?;
    if old.as_ref().is_some_and(|v| v.base_url != profile.base_url) && args.credential.is_none() {
        tx.execute(
            "DELETE FROM settings WHERE key=?1",
            [format!("ai_credential:{}", p.id)],
        )
        .map_err(storage_error)?;
    }
    if let Some(key) = args.credential {
        put(&tx, &format!("ai_credential:{}", p.id), key.trim())?;
    }
    put(
        &tx,
        &format!("ai_profile:{}", p.id),
        &serde_json::to_string(&profile).map_err(|_| AppError::invalid("providerProfile"))?,
    )?;
    put(&tx, &format!("ai_profile_name:{}", p.id), p.name.trim())?;
    put(
        &tx,
        &format!("ai_profile_revision:{}", p.id),
        &revision.to_string(),
    )?;
    let result = read(&tx, &p.id)?.ok_or_else(|| AppError::invalid("providerProfile"))?;
    tx.commit().map_err(storage_error)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_private_and_cannot_follow_endpoint_changes() {
        let path = std::env::temp_dir().join(format!("profiles-{}.db", uuid::Uuid::new_v4()));
        let p = ProviderEntry {
            id: "profile".into(),
            name: "Test".into(),
            base_url: "https://example.test/v1".into(),
            model: "model".into(),
            temperature: 0.2,
            max_output_tokens: 4096,
            timeout_seconds: 60,
            network_retries: 1,
            has_key: false,
            revision: Revision("0".into()),
        };
        let saved = save(
            &path,
            SaveProviderArgs {
                profile: p.clone(),
                credential: Some("test-secret".into()),
                expected_revision: None,
            },
        )
        .unwrap();
        let entries = list(&path).unwrap();
        assert!(entries[0].has_key);
        assert!(!serde_json::to_string(&entries)
            .unwrap()
            .contains("test-secret"));
        assert!(save(
            &path,
            SaveProviderArgs {
                profile: p,
                credential: None,
                expected_revision: None
            }
        )
        .is_err());
        let expected = Some(saved.revision.clone());
        let mut changed = saved;
        changed.base_url = "https://other.test/v1".into();
        let saved = save(
            &path,
            SaveProviderArgs {
                profile: changed,
                credential: None,
                expected_revision: expected,
            },
        )
        .unwrap();
        assert!(!saved.has_key);
        std::fs::remove_file(path).unwrap();
    }
}
