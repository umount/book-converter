//! Local capability checks only: no provider calls, downloads or job creation.
use crate::{
    ai::{ChatCompletions, ProviderProfile},
    app::contracts::{AppError, CapabilityRequirement, MangaPreflight, MangaStage, ProjectKind},
    application::runtime::provider_profile,
    storage::{repository::ProjectRepository, shared},
};
use rusqlite::Connection;

/// Shared by preflight and recognition admission; never falls back to the book profile.
pub(crate) fn recognition_provider(
    settings: &shared::ProcessingSettings,
) -> Result<(ProviderProfile, String), AppError> {
    let id = settings
        .manga_recognition_profile
        .as_deref()
        .ok_or_else(|| AppError::invalid("mangaRecognitionProfile"))?;
    checked_provider(id)
}
pub(crate) fn translation_provider(
    settings: &shared::ProcessingSettings,
) -> Result<(ProviderProfile, String), AppError> {
    checked_provider(
        settings
            .manga_translation_profile
            .as_deref()
            .ok_or_else(|| AppError::invalid("mangaTranslationProfile"))?,
    )
}
fn checked_provider(id: &str) -> Result<(ProviderProfile, String), AppError> {
    let (profile, key) = provider_profile(Some(id))?;
    ChatCompletions::new(profile.clone(), key.clone())?;
    Ok((profile, key))
}

pub fn inspect(db: &mut Connection, local: [Option<&str>; 3]) -> Result<MangaPreflight, AppError> {
    let mut result = inspect_with(db, |id| checked_provider(id).map(|_| ()))?;
    for (requirement, reason) in result.requirements[3..].iter_mut().zip(local) {
        requirement.available = reason.is_none();
        requirement.reason_key = reason.map(str::to_owned);
    }
    Ok(result)
}

fn inspect_with(
    db: &mut Connection,
    mut validate: impl FnMut(&str) -> Result<(), AppError>,
) -> Result<MangaPreflight, AppError> {
    ProjectRepository::new(db, ProjectKind::Manga)?;
    let settings = shared::settings(db)?.choices;
    let recognition = profile_issue(
        settings.manga_recognition_profile.as_deref(),
        "mangaRecognitionProfileRequired",
        &mut validate,
    );
    let translation = profile_issue(
        settings.manga_translation_profile.as_deref(),
        "mangaTranslationProfileRequired",
        &mut validate,
    );
    let requirement = |stage, reason: Option<&str>| CapabilityRequirement {
        stage,
        available: reason.is_none(),
        reason_key: reason.map(str::to_owned),
    };
    Ok(MangaPreflight {
        requirements: vec![
            requirement(MangaStage::Detection, recognition),
            requirement(MangaStage::Recognition, recognition),
            requirement(MangaStage::Translation, translation),
            requirement(MangaStage::Masks, Some("mangaMasksUnavailable")),
            // Downloaded weights are not a tested inference adapter. Do not suggest that
            // downloading the experimental catalog candidate makes processing ready.
            requirement(MangaStage::Inpainting, Some("mangaInpaintingUnavailable")),
            requirement(MangaStage::Lettering, Some("mangaLetteringUnavailable")),
        ],
    })
}
fn profile_issue<'a>(
    selected: Option<&str>,
    missing: &'a str,
    validate: &mut impl FnMut(&str) -> Result<(), AppError>,
) -> Option<&'a str> {
    let Some(id) = selected else {
        return Some(missing);
    };
    match validate(id) {
        Ok(()) => None,
        Err(error) if error.message_key == "errors.apiKeyRequired" => {
            Some("mangaProfileKeyRequired")
        }
        Err(_) => Some("mangaProfileInvalid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::contracts::ErrorCode;
    use crate::storage::tests::database;

    #[test]
    fn missing_manga_profiles_never_use_book_defaults_or_change_the_project() {
        let mut db = database(ProjectKind::Manga);
        db.execute(
            "UPDATE project_settings SET profiles_json='{\"book_translation\":\"book\"}'",
            [],
        )
        .unwrap();
        let before = db.total_changes();
        let result = inspect_with(&mut db, |_| {
            panic!("must not resolve an unselected profile")
        })
        .unwrap();
        assert!(!result.ready());
        assert_eq!(result.requirements.len(), 6);
        assert_eq!(
            result.requirements[0].reason_key.as_deref(),
            Some("mangaRecognitionProfileRequired")
        );
        assert_eq!(
            result.requirements[2].reason_key.as_deref(),
            Some("mangaTranslationProfileRequired")
        );
        assert_eq!(db.total_changes(), before);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM job_runs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn valid_profiles_do_not_enable_missing_processing_adapters() {
        let mut db = database(ProjectKind::Manga);
        db.execute("UPDATE project_settings SET profiles_json='{\"manga_recognition\":\"vision\",\"manga_translation\":\"dialogue\"}'", []).unwrap();
        let mut checked = Vec::new();
        let result = inspect_with(&mut db, |id| {
            checked.push(id.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(checked, ["vision", "dialogue"]);
        assert!(result.requirements[0].available && result.requirements[1].available);
        assert!(result.requirements[2].available);
        assert!(result.requirements[3..].iter().all(|r| !r.available));
        assert!(!result.ready());
        let result = inspect_with(&mut db, |_| {
            Err(AppError {
                code: ErrorCode::CapabilityUnavailable,
                message_key: "errors.apiKeyRequired".into(),
                params: Default::default(),
                retryable: false,
            })
        })
        .unwrap();
        assert_eq!(
            result.requirements[1].reason_key.as_deref(),
            Some("mangaProfileKeyRequired")
        );
        let result = inspect_with(&mut db, |_| Err(AppError::invalid("providerUrl"))).unwrap();
        assert_eq!(
            result.requirements[1].reason_key.as_deref(),
            Some("mangaProfileInvalid")
        );
    }

    #[test]
    fn book_projects_cannot_use_manga_preflight() {
        let mut db = database(ProjectKind::Book);
        assert_eq!(
            inspect_with(&mut db, |_| panic!("wrong domain"))
                .unwrap_err()
                .code,
            ErrorCode::WrongProjectKind
        );
    }
}
