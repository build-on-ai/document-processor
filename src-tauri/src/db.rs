use crate::parser::ProcessedDocument;
use crate::Stats;
use rusqlite::{Connection, Result, params};
use std::path::Path;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Explicit, not relying on the bundled SQLite build's compile-time
        // default: FK enforcement (and therefore ON DELETE CASCADE below)
        // is off by default in SQLite unless the library was built with
        // SQLITE_DEFAULT_FOREIGN_KEYS=1. Must run before creating tables
        // that declare foreign keys — SQLite reads this pragma per
        // connection, not per statement.
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS documents (
                id TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                original_path TEXT NOT NULL,
                doc_type TEXT,
                classification_confidence REAL,
                pages INTEGER,
                word_count INTEGER,
                size INTEGER,
                full_text TEXT,
                text_preview TEXT,
                metadata TEXT,
                processed_at TEXT NOT NULL,
                status TEXT DEFAULT 'processed'
            );

            CREATE TABLE IF NOT EXISTS images (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                filename TEXT NOT NULL,
                page INTEGER,
                position_marker TEXT,
                context_before TEXT,
                context_after TEXT,
                ocr_text TEXT,
                ai_description TEXT,
                image_path TEXT,
                thumbnail_path TEXT,
                width INTEGER,
                height INTEGER,
                FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_documents_processed_at ON documents(processed_at DESC);
            CREATE INDEX IF NOT EXISTS idx_documents_doc_type ON documents(doc_type);
            CREATE INDEX IF NOT EXISTS idx_images_document_id ON images(document_id);
            "#,
        )?;

        Ok(Self { conn })
    }

    pub fn document_exists(&self, original_path: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE original_path = ?1",
            params![original_path],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn clear_duplicates(&self) -> Result<usize> {
        // Keep only the newest entry for each original_path
        let deleted = self.conn.execute(
            r#"
            DELETE FROM documents WHERE id NOT IN (
                SELECT id FROM documents d1
                WHERE processed_at = (
                    SELECT MAX(processed_at) FROM documents d2
                    WHERE d2.original_path = d1.original_path
                )
            )
            "#,
            [],
        )?;
        Ok(deleted)
    }

    /// Persists `doc`, returning the id the row was actually saved under.
    ///
    /// On reprocess (a document already exists for `doc.original_path`),
    /// that existing row's id is reused — `doc.id` itself is a fresh id
    /// generated for this processing run and is NOT what ends up as the
    /// primary key. Callers that hand `doc` back to the frontend as-is
    /// after saving must overwrite `doc.id` with the returned value, or
    /// the frontend ends up holding an id that has no corresponding row
    /// (`get_document` on it 404s).
    ///
    /// Uses `unchecked_transaction` (works on `&self`, unlike
    /// `transaction` which needs `&mut self`) so the document row, the
    /// old image rows being cleared, and the new image rows all commit
    /// or roll back together — a failure partway through the image loop
    /// no longer leaves the document row pointing at a partial image set.
    pub fn save_document(&self, doc: &ProcessedDocument) -> Result<String> {
        // Check for duplicate by path - update existing instead of creating new
        let existing_id: Option<String> = self.conn.query_row(
            "SELECT id FROM documents WHERE original_path = ?1",
            params![doc.original_path],
            |row| row.get(0),
        ).ok();

        let doc_id = existing_id.unwrap_or_else(|| doc.id.clone());

        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            r#"
            INSERT OR REPLACE INTO documents
            (id, filename, original_path, doc_type, classification_confidence,
             pages, word_count, size, full_text, text_preview, metadata, processed_at, status)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                doc_id,
                doc.filename,
                doc.original_path,
                doc.doc_type,
                doc.classification_confidence,
                doc.pages,
                doc.word_count,
                doc.size,
                doc.full_text,
                doc.text_preview,
                serde_json::to_string(&doc.metadata).unwrap_or_default(),
                doc.processed_at,
                "processed"
            ],
        )?;

        // Reprocessing replaces the image set wholesale: without this,
        // a document's old images (from a previous pass, possibly for a
        // page/image count that no longer matches) would linger forever
        // under the same document_id instead of being replaced.
        tx.execute("DELETE FROM images WHERE document_id = ?1", params![doc_id])?;

        for img in &doc.images {
            tx.execute(
                r#"
                INSERT INTO images
                (id, document_id, filename, page, position_marker,
                 context_before, context_after, ocr_text, ai_description,
                 image_path, thumbnail_path, width, height)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                params![
                    img.id,
                    doc_id,
                    img.filename,
                    img.page,
                    img.position_marker,
                    img.context_before,
                    img.context_after,
                    img.ocr_text,
                    img.ai_description,
                    img.image_path,
                    img.thumbnail_path,
                    img.width,
                    img.height,
                ],
            )?;
        }

        tx.commit()?;

        Ok(doc_id)
    }

    pub fn get_document(&self, id: &str) -> Result<ProcessedDocument> {
        let mut stmt = self.conn.prepare(
            "SELECT id, filename, original_path, doc_type, classification_confidence,
                    pages, word_count, size, full_text, text_preview, metadata, processed_at
             FROM documents WHERE id = ?1",
        )?;

        let doc = stmt.query_row(params![id], |row| {
            let metadata_str: String = row.get(10)?;
            let metadata = serde_json::from_str(&metadata_str).unwrap_or_default();

            Ok(ProcessedDocument {
                id: row.get(0)?,
                filename: row.get(1)?,
                original_path: row.get(2)?,
                doc_type: row.get(3)?,
                classification_confidence: row.get(4)?,
                pages: row.get(5)?,
                word_count: row.get(6)?,
                size: row.get(7)?,
                full_text: row.get(8)?,
                text_preview: row.get(9)?,
                metadata,
                processed_at: row.get(11)?,
                images: vec![],
            })
        })?;

        // Load images
        let mut doc = doc;
        let mut img_stmt = self.conn.prepare(
            "SELECT id, filename, page, position_marker, context_before, context_after,
                    ocr_text, ai_description, image_path, thumbnail_path, width, height
             FROM images WHERE document_id = ?1",
        )?;

        let images = img_stmt.query_map(params![id], |row| {
            Ok(crate::parser::ExtractedImage {
                id: row.get(0)?,
                filename: row.get(1)?,
                page: row.get(2)?,
                position_marker: row.get(3)?,
                context_before: row.get(4)?,
                context_after: row.get(5)?,
                ocr_text: row.get(6)?,
                ai_description: row.get(7)?,
                image_path: row.get(8)?,
                thumbnail_path: row.get(9)?,
                width: row.get(10)?,
                height: row.get(11)?,
            })
        })?;

        doc.images = images.filter_map(|r| r.ok()).collect();

        Ok(doc)
    }

    pub fn get_recent_documents(&self, limit: u32) -> Result<Vec<ProcessedDocument>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, filename, original_path, doc_type, classification_confidence,
                    pages, word_count, size, NULL, text_preview, metadata, processed_at
             FROM documents
             ORDER BY processed_at DESC
             LIMIT ?1",
        )?;

        let docs = stmt.query_map(params![limit], |row| {
            let metadata_str: String = row.get(10)?;
            let metadata = serde_json::from_str(&metadata_str).unwrap_or_default();

            Ok(ProcessedDocument {
                id: row.get(0)?,
                filename: row.get(1)?,
                original_path: row.get(2)?,
                doc_type: row.get(3)?,
                classification_confidence: row.get(4)?,
                pages: row.get(5)?,
                word_count: row.get(6)?,
                size: row.get(7)?,
                full_text: None,
                text_preview: row.get(9)?,
                metadata,
                processed_at: row.get(11)?,
                images: vec![],
            })
        })?;

        Ok(docs.filter_map(|r| r.ok()).collect())
    }

    pub fn get_stats(&self) -> Result<Stats> {
        let total: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM documents",
            [],
            |row| row.get(0),
        )?;

        let processed: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE status = 'processed'",
            [],
            |row| row.get(0),
        )?;

        let failed: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE status = 'failed'",
            [],
            |row| row.get(0),
        )?;

        Ok(Stats { total, processed, failed })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let result = stmt.query_row(params![key], |row| row.get(0));

        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn delete_all_documents(&self) -> Result<usize> {
        // First delete all images
        self.conn.execute("DELETE FROM images", [])?;
        // Then delete all documents
        let deleted = self.conn.execute("DELETE FROM documents", [])?;
        Ok(deleted)
    }

    pub fn update_document_type(&self, id: &str, doc_type: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE documents SET doc_type = ?1, classification_confidence = 1.0 WHERE id = ?2",
            params![doc_type, id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ExtractedImage;
    use chrono::Utc;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn doc_with_images(original_path: &str, n_images: usize) -> ProcessedDocument {
        ProcessedDocument {
            id: Uuid::new_v4().to_string(),
            filename: "test.pdf".to_string(),
            original_path: original_path.to_string(),
            doc_type: Some("umowa".to_string()),
            classification_confidence: Some(0.9),
            pages: Some(1),
            word_count: Some(10),
            size: 100,
            full_text: Some("tresc".to_string()),
            text_preview: Some("tresc".to_string()),
            metadata: HashMap::new(),
            processed_at: Utc::now().to_rfc3339(),
            images: (0..n_images)
                .map(|i| ExtractedImage {
                    id: Uuid::new_v4().to_string(),
                    filename: format!("img_{i}.png"),
                    page: Some(1),
                    position_marker: Some(format!("obj_{i}")),
                    context_before: None,
                    context_after: None,
                    ocr_text: None,
                    ai_description: None,
                    image_path: Some(format!("/tmp/img_{i}.png")),
                    thumbnail_path: None,
                    width: Some(10),
                    height: Some(10),
                })
                .collect(),
        }
    }

    fn temp_db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(&dir.path().join("test.sqlite")).unwrap();
        (dir, db)
    }

    // CRITICAL #1 repro (Report 3/4): reprocessing a document that has
    // images used to hit SqliteFailure(787, "FOREIGN KEY constraint
    // failed") because the image rows were inserted with document_id =
    // doc.id (a *fresh* id generated for this run) instead of the
    // resolved doc_id (the existing row's actual primary key). This is
    // the exact "Skanuj ponownie" crash from the audit, reproduced
    // in-process rather than by re-reading the original bug report.
    #[test]
    fn reprocessing_document_with_images_does_not_crash() {
        let (_dir, db) = temp_db();
        let doc_v1 = doc_with_images("/docs/contract.pdf", 2);

        let id_v1 = db.save_document(&doc_v1).expect("first save must succeed");

        // Second pass over the same original_path: a fresh ProcessedDocument
        // (new doc.id, new image ids — exactly what re-running the parser
        // on the same file produces) must resolve to the SAME row, not crash.
        let doc_v2 = doc_with_images("/docs/contract.pdf", 3);
        let id_v2 = db
            .save_document(&doc_v2)
            .expect("reprocess with images must not hit a FK violation");

        assert_eq!(id_v1, id_v2, "reprocess must reuse the existing document's id");
        assert_ne!(id_v2, doc_v2.id, "returned id must be the resolved one, not doc.id");

        let stored = db.get_document(&id_v2).expect("row must exist under the returned id");
        assert_eq!(stored.original_path, "/docs/contract.pdf");
    }

    // The no-images variant from the same finding: save_document must
    // return an id the caller can actually look up afterwards.
    #[test]
    fn save_document_returns_a_resolvable_id() {
        let (_dir, db) = temp_db();
        let doc = doc_with_images("/docs/no_images.pdf", 0);
        let doc_id = doc.id.clone();

        let returned = db.save_document(&doc).unwrap();
        assert_eq!(returned, doc_id, "first save: no existing row, so id is doc.id");
        db.get_document(&returned).expect("returned id must resolve");
    }

    // Reprocessing must replace the image set, not accumulate rows from
    // every previous pass under the same document_id.
    #[test]
    fn reprocessing_replaces_stale_images_instead_of_accumulating() {
        let (_dir, db) = temp_db();
        let doc_v1 = doc_with_images("/docs/shrinking.pdf", 3);
        let id = db.save_document(&doc_v1).unwrap();

        let count_after_v1: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM images WHERE document_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_after_v1, 3);

        let doc_v2 = doc_with_images("/docs/shrinking.pdf", 1);
        db.save_document(&doc_v2).unwrap();

        let count_after_v2: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM images WHERE document_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_after_v2, 1, "old images must be cleared, not kept alongside the new set");
    }
}
