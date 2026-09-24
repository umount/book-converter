//! Reconcile automatic detections without losing human-authored text.
use super::recognition::{RecognizedRegion, RegionCategory};
use crate::{
    app::contracts::{AppError, PixelBounds},
    storage::repository::storage_error,
};
use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub id: String,
    pub reading_order: u32,
    pub category: RegionCategory,
    pub bounds: PixelBounds,
    pub source_text: String,
    pub translated_text: Option<String>,
    pub source_manual: bool,
    pub translation_manual: bool,
    pub revision: i64,
}
pub fn read(db: &rusqlite::Connection, page: &str) -> Result<Vec<Region>, AppError> {
    let mut query = db.prepare("SELECT id,reading_order,category,geometry_json,source_text,translated_text,source_manual,translation_manual,revision FROM manga_regions WHERE page_id=?1 ORDER BY reading_order").map_err(storage_error)?;
    let rows = query
        .query_map([page], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, bool>(6)?,
                r.get::<_, bool>(7)?,
                r.get::<_, i64>(8)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    rows.into_iter()
        .map(
            |(
                id,
                reading_order,
                category,
                geometry,
                source_text,
                translated_text,
                source_manual,
                translation_manual,
                revision,
            )| {
                Ok(Region {
                    id,
                    reading_order,
                    category: serde_json::from_value(serde_json::Value::String(category))
                        .map_err(|_| AppError::invalid("regionCategory"))?,
                    bounds: serde_json::from_str(&geometry)
                        .map_err(|_| AppError::invalid("bounds"))?,
                    source_text,
                    translated_text,
                    source_manual,
                    translation_manual,
                    revision,
                })
            },
        )
        .collect()
}
fn overlap(a: &PixelBounds, b: &PixelBounds) -> f64 {
    let width = ((a.x + a.width).min(b.x + b.width) - a.x.max(b.x)).max(0.0);
    let height = ((a.y + a.height).min(b.y + b.height) - a.y.max(b.y)).max(0.0);
    let intersection = width * height;
    intersection / (a.width * a.height + b.width * b.height - intersection)
}

/// Only unambiguous mutual overlaps inherit durable IDs. Unmatched edits stay visible.
pub fn reconcile(
    tx: &Transaction<'_>,
    page: &str,
    proposed: &[RecognizedRegion],
) -> Result<(Vec<Region>, Vec<String>), AppError> {
    let old = read(tx, page)?;
    let matches: Vec<Vec<usize>> = proposed
        .iter()
        .map(|new| {
            old.iter()
                .enumerate()
                .filter(|(_, prev)| overlap(&new.bounds, &prev.bounds) >= 0.8)
                .map(|(i, _)| i)
                .collect()
        })
        .collect();
    let mut used = HashSet::new();
    let mut issues = Vec::new();
    // Move old positions out of the way without violating the UNIQUE page/order index.
    let offset = old
        .iter()
        .map(|r| i64::from(r.reading_order))
        .max()
        .unwrap_or(0)
        + i64::try_from(old.len() + proposed.len() + 1)
            .map_err(|_| AppError::invalid("regionCount"))?;
    tx.execute(
        "UPDATE manga_regions SET reading_order=reading_order+?2 WHERE page_id=?1",
        params![page, offset],
    )
    .map_err(storage_error)?;
    for (position, new) in proposed.iter().enumerate() {
        let matched = matches[position].first().copied().filter(|candidate| {
            matches[position].len() == 1
                && matches
                    .iter()
                    .filter(|items| items.contains(candidate))
                    .count()
                    == 1
        });
        let geometry =
            serde_json::to_string(&new.bounds).map_err(|_| AppError::invalid("bounds"))?;
        let category =
            serde_json::to_value(&new.category).map_err(|_| AppError::invalid("regionCategory"))?;
        if let Some(index) = matched {
            used.insert(index);
            let previous = &old[index];
            let source = if previous.source_manual {
                &previous.source_text
            } else {
                &new.source_text
            };
            let changed = source != &previous.source_text;
            let translated = if changed && !previous.translation_manual {
                None
            } else {
                previous.translated_text.as_deref()
            };
            if changed && previous.translation_manual {
                issues.push(format!("translation_source_changed:{}", previous.id));
            }
            tx.execute("UPDATE manga_regions SET reading_order=?2,category=?3,geometry_json=?4,source_text=?5,translated_text=?6,revision=revision+1,text_revision=text_revision+?7,geometry_revision=geometry_revision+?8 WHERE id=?1",params![previous.id,position,category.as_str(),geometry,source,translated,i64::from(changed),i64::from(previous.bounds!=new.bounds)]).map_err(storage_error)?;
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute("INSERT INTO manga_regions(id,page_id,reading_order,category,geometry_json,source_text) VALUES(?1,?2,?3,?4,?5,?6)",params![id,page,position,category.as_str(),geometry,new.source_text]).map_err(storage_error)?;
        }
    }
    let mut position = proposed.len();
    for (_, previous) in old
        .iter()
        .enumerate()
        .filter(|(index, _)| !used.contains(index))
    {
        if previous.source_manual || previous.translation_manual {
            tx.execute(
                "UPDATE manga_regions SET reading_order=?2,revision=revision+1 WHERE id=?1",
                params![previous.id, position],
            )
            .map_err(storage_error)?;
            position += 1;
            issues.push(format!("unmatched_manual_region:{}", previous.id));
        } else {
            tx.execute("DELETE FROM manga_regions WHERE id=?1", [&previous.id])
                .map_err(storage_error)?;
        }
    }
    tx.execute("DELETE FROM manga_masks WHERE page_id=?1", [page])
        .map_err(storage_error)?;
    Ok((read(tx, page)?, issues))
}
