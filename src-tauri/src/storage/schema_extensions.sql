CREATE TABLE IF NOT EXISTS book_presentation (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    title TEXT, author TEXT, summary TEXT,
    instructions TEXT NOT NULL DEFAULT '',
    cover_asset_id TEXT REFERENCES assets(id),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision)='integer' AND revision>=0)
);
CREATE TRIGGER IF NOT EXISTS book_presentation_kind_guard BEFORE INSERT ON book_presentation
WHEN (SELECT kind FROM project_settings WHERE singleton=1)!='book'
BEGIN SELECT RAISE(ABORT, 'Book presentation requires book project'); END;
CREATE INDEX IF NOT EXISTS job_steps_chapter_lookup ON job_steps(entity_kind,entity_id);
CREATE VIEW IF NOT EXISTS book_chapter_states AS
SELECT c.id,c.position,c.source_title,c.revision,
 CASE WHEN s.state IN ('running','cancelling') THEN 'in_progress'
      WHEN s.state='failed' THEN 'failed'
      WHEN t.id IS NOT NULL THEN 'done'
      WHEN NOT EXISTS(SELECT 1 FROM book_source_blocks b WHERE b.chapter_id=c.id AND b.kind IN ('text','caption') AND trim(b.text)!='') THEN 'skipped'
      ELSE 'pending' END AS status,
 CASE WHEN t.id IS NULL THEN NULL WHEN t.provenance='reference' THEN 'reference'
      WHEN t.provenance IN ('manual','manual-replace') THEN 'manual' ELSE 'model' END AS origin,
 COALESCE(t.status!='ready',0) AS needs_review,
 s.error AS translation_error
FROM book_chapters c
LEFT JOIN book_translations t ON t.id=(SELECT id FROM book_translations WHERE chapter_id=c.id AND target_language=(SELECT target_language FROM project_settings WHERE singleton=1) ORDER BY revision DESC LIMIT 1)
LEFT JOIN job_steps s ON s.rowid=(SELECT rowid FROM job_steps WHERE entity_kind='chapter' AND entity_id=c.id AND stage IN ('translation','glossary','context') ORDER BY rowid DESC LIMIT 1);

CREATE TABLE IF NOT EXISTS book_source_metadata (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), title TEXT, author TEXT, summary TEXT
);
CREATE TRIGGER IF NOT EXISTS book_source_metadata_kind_guard BEFORE INSERT ON book_source_metadata
WHEN (SELECT kind FROM project_settings WHERE singleton=1)!='book'
BEGIN SELECT RAISE(ABORT, 'Book metadata requires book project'); END;

CREATE TABLE IF NOT EXISTS book_volume_titles (
    source TEXT NOT NULL,
    target_language TEXT NOT NULL,
    title TEXT NOT NULL,
    revision INTEGER NOT NULL,
    PRIMARY KEY(source, target_language)
);
