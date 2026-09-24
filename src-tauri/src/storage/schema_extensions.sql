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
CREATE TABLE IF NOT EXISTS manga_page_previews (
    page_id TEXT PRIMARY KEY NOT NULL REFERENCES manga_pages(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES assets(id)
);
