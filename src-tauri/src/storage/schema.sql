-- Version 1: independent book and manga domains with shared execution metadata.
CREATE TABLE project_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    kind TEXT NOT NULL CHECK(kind IN ('book', 'manga')),
    source_language TEXT,
    languages_locked INTEGER NOT NULL DEFAULT 1 CHECK(languages_locked IN (0,1)),
    target_language TEXT NOT NULL CHECK(length(target_language) > 0),
    profiles_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(profiles_json)),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision) = 'integer' AND revision >= 0)
);
CREATE TABLE assets (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 64 AND id NOT GLOB '*[^0-9a-f]*'),
    relative_path TEXT NOT NULL UNIQUE,
    mime TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK(byte_length > 0),
    width INTEGER CHECK(width > 0), height INTEGER CHECK(height > 0)
);
CREATE TABLE glossary_state (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision) = 'integer' AND revision >= 0));
INSERT INTO glossary_state(singleton) VALUES(1);
CREATE TABLE glossary_terms (
    id TEXT PRIMARY KEY NOT NULL, source TEXT NOT NULL UNIQUE, target TEXT NOT NULL,
    kind TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0 CHECK(pinned IN (0,1)),
    frequency INTEGER NOT NULL DEFAULT 0 CHECK(frequency >= 0), revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision) = 'integer' AND revision >= 0)
);
CREATE TABLE job_runs (
    id TEXT PRIMARY KEY NOT NULL, kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('queued','running','succeeded','failed','cancelling','cancelled','interrupted')),
    settings_snapshot TEXT NOT NULL CHECK(json_valid(settings_snapshot)),
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL, terminal_error TEXT,
    revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision) = 'integer' AND revision >= 0)
);
CREATE UNIQUE INDEX one_active_mutation ON job_runs((1)) WHERE state IN ('queued','running','cancelling');
CREATE TABLE job_steps (
    id TEXT PRIMARY KEY NOT NULL, run_id TEXT NOT NULL REFERENCES job_runs(id) ON DELETE CASCADE,
    entity_kind TEXT NOT NULL, entity_id TEXT NOT NULL, stage TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK(attempt > 0), input_fingerprint TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('queued','running','succeeded','failed','cancelling','cancelled','interrupted')),
    output_reference TEXT, duration_ms INTEGER CHECK(duration_ms >= 0), error TEXT,
    UNIQUE(run_id, entity_kind, entity_id, stage, attempt)
);
CREATE INDEX job_steps_run ON job_steps(run_id,state);
CREATE TABLE assistant_messages (
    id TEXT PRIMARY KEY NOT NULL, position INTEGER NOT NULL UNIQUE, role TEXT NOT NULL,
    content TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE book_chapters (
    id TEXT PRIMARY KEY NOT NULL, position INTEGER NOT NULL UNIQUE CHECK(position >= 0),
    display_number INTEGER, source_title TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision) = 'integer' AND revision >= 0), instructions TEXT NOT NULL DEFAULT ''
);
CREATE TABLE book_source_blocks (
    id TEXT PRIMARY KEY NOT NULL, chapter_id TEXT NOT NULL REFERENCES book_chapters(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0), kind TEXT NOT NULL CHECK(kind IN ('text','caption','image')),
    text TEXT, asset_id TEXT REFERENCES assets(id) ON DELETE RESTRICT,
    alt TEXT NOT NULL DEFAULT '', revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision) = 'integer' AND revision >= 0),
    UNIQUE(chapter_id, position), UNIQUE(id, chapter_id),
    CHECK((kind IN ('text','caption') AND text IS NOT NULL AND asset_id IS NULL)
       OR (kind = 'image' AND text IS NULL AND asset_id IS NOT NULL))
);
CREATE TABLE book_translations (
    id TEXT PRIMARY KEY NOT NULL, chapter_id TEXT NOT NULL REFERENCES book_chapters(id) ON DELETE CASCADE,
    source_revision INTEGER NOT NULL CHECK(typeof(source_revision) = 'integer' AND source_revision >= 0), settings_revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(settings_revision) = 'integer' AND settings_revision >= 0), status TEXT NOT NULL CHECK(status IN ('ready','stale','needs_review')),
    provenance TEXT NOT NULL, target_language TEXT NOT NULL, translated_title TEXT NOT NULL,
    context_fingerprint TEXT NOT NULL, glossary_revision INTEGER NOT NULL CHECK(typeof(glossary_revision) = 'integer' AND glossary_revision >= 0),
    revision INTEGER NOT NULL CHECK(typeof(revision) = 'integer' AND revision >= 0), UNIQUE(id, chapter_id), UNIQUE(chapter_id, target_language, revision)
);
CREATE TABLE book_translation_blocks (
    translation_id TEXT NOT NULL, chapter_id TEXT NOT NULL, source_block_id TEXT NOT NULL,
    translated_text TEXT NOT NULL,
    PRIMARY KEY(translation_id,source_block_id),
    FOREIGN KEY(translation_id,chapter_id) REFERENCES book_translations(id,chapter_id) ON DELETE CASCADE,
    FOREIGN KEY(source_block_id,chapter_id) REFERENCES book_source_blocks(id,chapter_id) ON DELETE CASCADE
);
CREATE TABLE book_contexts (
    id TEXT PRIMARY KEY NOT NULL, translation_id TEXT NOT NULL UNIQUE REFERENCES book_translations(id) ON DELETE CASCADE,
    summary TEXT NOT NULL, previous_tail TEXT NOT NULL,
    translation_revision INTEGER NOT NULL CHECK(typeof(translation_revision) = 'integer' AND translation_revision >= 0),
    predecessor_id TEXT REFERENCES book_contexts(id) ON DELETE SET NULL
);
CREATE TABLE book_reference_chapters (id TEXT PRIMARY KEY NOT NULL, position INTEGER NOT NULL UNIQUE, title TEXT NOT NULL, text TEXT NOT NULL);
CREATE TABLE book_reference_mappings (
    chapter_id TEXT PRIMARY KEY NOT NULL REFERENCES book_chapters(id) ON DELETE CASCADE,
    reference_id TEXT NOT NULL REFERENCES book_reference_chapters(id) ON DELETE CASCADE
);
CREATE TABLE manga_volumes (
    id TEXT PRIMARY KEY NOT NULL, position INTEGER NOT NULL UNIQUE CHECK(position >= 0), title TEXT NOT NULL,
    reading_direction TEXT NOT NULL CHECK(reading_direction IN ('rtl','ltr'))
);
CREATE TABLE manga_pages (
    id TEXT PRIMARY KEY NOT NULL, volume_id TEXT NOT NULL REFERENCES manga_volumes(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    original_asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE RESTRICT,
    width INTEGER NOT NULL CHECK(width > 0), height INTEGER NOT NULL CHECK(height > 0),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision) = 'integer' AND revision >= 0), UNIQUE(volume_id,position)
);
CREATE TABLE manga_regions (
    id TEXT PRIMARY KEY NOT NULL, page_id TEXT NOT NULL REFERENCES manga_pages(id) ON DELETE CASCADE,
    reading_order INTEGER NOT NULL CHECK(reading_order >= 0),
    category TEXT NOT NULL CHECK(category IN ('dialogue','narration','sfx')),
    geometry_json TEXT NOT NULL CHECK(json_valid(geometry_json)),
    source_text TEXT NOT NULL DEFAULT '', translated_text TEXT,
    text_revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(text_revision) = 'integer' AND text_revision >= 0), style_revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(style_revision) = 'integer' AND style_revision >= 0),
    style_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(style_json)),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision) = 'integer' AND revision >= 0), geometry_revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(geometry_revision) = 'integer' AND geometry_revision >= 0),
    source_manual INTEGER NOT NULL DEFAULT 0 CHECK(source_manual IN (0,1)),
    translation_manual INTEGER NOT NULL DEFAULT 0 CHECK(translation_manual IN (0,1)),
    UNIQUE(page_id,reading_order), UNIQUE(id,page_id)
);
CREATE TABLE manga_masks (
    id TEXT PRIMARY KEY NOT NULL, page_id TEXT NOT NULL REFERENCES manga_pages(id) ON DELETE CASCADE,
    region_id TEXT, asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE RESTRICT,
    geometry_revision INTEGER NOT NULL CHECK(typeof(geometry_revision) = 'integer' AND geometry_revision >= 0),
    FOREIGN KEY(region_id,page_id) REFERENCES manga_regions(id,page_id) ON DELETE CASCADE
);
CREATE TABLE manga_results (
    id TEXT PRIMARY KEY NOT NULL, page_id TEXT NOT NULL REFERENCES manga_pages(id) ON DELETE CASCADE,
    stage TEXT NOT NULL CHECK(stage IN ('detection','recognition','translation','masks','inpainting','lettering')),
    input_fingerprint TEXT NOT NULL CHECK(length(input_fingerprint) > 0), revision INTEGER NOT NULL CHECK(typeof(revision) = 'integer' AND revision >= 0),
    page_revision INTEGER NOT NULL CHECK(typeof(page_revision) = 'integer' AND page_revision >= 0), settings_revision INTEGER NOT NULL CHECK(typeof(settings_revision) = 'integer' AND settings_revision >= 0), glossary_revision INTEGER NOT NULL CHECK(typeof(glossary_revision) = 'integer' AND glossary_revision >= 0),
    output_asset_id TEXT REFERENCES assets(id) ON DELETE RESTRICT, payload_json TEXT CHECK(json_valid(payload_json)),
    provider_version TEXT NOT NULL, validity TEXT NOT NULL CHECK(validity IN ('current','stale')),
    CHECK(output_asset_id IS NOT NULL OR payload_json IS NOT NULL), UNIQUE(page_id,stage,revision)
);
CREATE TABLE manga_reviews (
    result_id TEXT PRIMARY KEY NOT NULL REFERENCES manga_results(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK(state IN ('unreviewed','needs_review','approved')),
    issues_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(issues_json))
);
CREATE TRIGGER book_kind_guard BEFORE INSERT ON book_chapters
WHEN COALESCE((SELECT kind FROM project_settings WHERE singleton = 1),'') != 'book'
BEGIN SELECT RAISE(ABORT, 'wrong_project_kind'); END;
CREATE TRIGGER manga_kind_guard BEFORE INSERT ON manga_volumes
WHEN COALESCE((SELECT kind FROM project_settings WHERE singleton = 1),'') != 'manga'
BEGIN SELECT RAISE(ABORT, 'wrong_project_kind'); END;
-- Project domain and immutable pixel identities cannot change after publication.
CREATE TRIGGER immutable_project_kind BEFORE UPDATE OF kind ON project_settings
WHEN NEW.kind != OLD.kind
BEGIN SELECT RAISE(ABORT, 'immutable_project_kind'); END;
CREATE TRIGGER immutable_asset_metadata BEFORE UPDATE ON assets
BEGIN SELECT RAISE(ABORT, 'immutable_asset_metadata'); END;
CREATE TRIGGER immutable_page_original BEFORE UPDATE OF original_asset_id,width,height ON manga_pages
WHEN NEW.original_asset_id != OLD.original_asset_id OR NEW.width != OLD.width OR NEW.height != OLD.height
BEGIN SELECT RAISE(ABORT, 'immutable_page_original'); END;
CREATE TRIGGER reference_kind_guard BEFORE INSERT ON book_reference_chapters
WHEN COALESCE((SELECT kind FROM project_settings WHERE singleton = 1),'') != 'book'
BEGIN SELECT RAISE(ABORT, 'wrong_project_kind'); END;
CREATE TRIGGER translation_block_kind_guard BEFORE INSERT ON book_translation_blocks
WHEN COALESCE((SELECT kind FROM book_source_blocks WHERE id=NEW.source_block_id),'') NOT IN ('text','caption')
BEGIN SELECT RAISE(ABORT, 'not_a_text_block'); END;
CREATE TRIGGER translation_block_update_guard BEFORE UPDATE ON book_translation_blocks
WHEN COALESCE((SELECT kind FROM book_source_blocks WHERE id=NEW.source_block_id),'') NOT IN ('text','caption')
BEGIN SELECT RAISE(ABORT, 'not_a_text_block'); END;
CREATE INDEX translation_chapter ON book_translations(chapter_id, target_language, revision DESC);
CREATE INDEX result_page ON manga_results(page_id, stage, revision DESC);
PRAGMA user_version = 1;

-- Independent metadata results; historical jobs keep their immutable output reference.
CREATE TABLE book_metadata (
    id TEXT PRIMARY KEY NOT NULL, input_fingerprint TEXT NOT NULL,
    title TEXT NOT NULL, author TEXT NOT NULL, summary TEXT NOT NULL
);
CREATE TRIGGER metadata_kind_guard BEFORE INSERT ON book_metadata
WHEN (SELECT kind FROM project_settings WHERE singleton=1)!='book'
BEGIN SELECT RAISE(ABORT, 'book metadata in manga project'); END;

CREATE TRIGGER project_languages_immutable BEFORE UPDATE OF source_language,target_language ON project_settings
WHEN OLD.languages_locked=1 AND (NEW.source_language IS NOT OLD.source_language OR NEW.target_language IS NOT OLD.target_language)
BEGIN SELECT RAISE(ABORT, 'project languages are immutable'); END;
CREATE TRIGGER project_languages_cannot_unlock BEFORE UPDATE OF languages_locked ON project_settings
WHEN OLD.languages_locked=1 AND NEW.languages_locked!=1
BEGIN SELECT RAISE(ABORT, 'project languages cannot be unlocked'); END;
CREATE TABLE book_glossary_results (
    id TEXT PRIMARY KEY NOT NULL, chapter_id TEXT NOT NULL REFERENCES book_chapters(id),
    source_revision INTEGER NOT NULL, settings_revision INTEGER NOT NULL,
    terms_json TEXT NOT NULL CHECK(json_valid(terms_json))
);
CREATE TRIGGER glossary_result_kind_guard BEFORE INSERT ON book_glossary_results
WHEN (SELECT kind FROM project_settings WHERE singleton=1)!='book'
BEGIN SELECT RAISE(ABORT, 'book glossary result in manga project'); END;
CREATE TABLE book_term_occurrences (
    chapter_id TEXT NOT NULL REFERENCES book_chapters(id) ON DELETE CASCADE,
    source TEXT NOT NULL, frequency INTEGER NOT NULL CHECK(frequency > 0),
    PRIMARY KEY(chapter_id,source)
);
