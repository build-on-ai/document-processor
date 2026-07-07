// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod db;
mod parser;
mod watcher;

use db::Database;
use parser::{DocumentProcessor, ProcessedDocument};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

struct AppState {
    db: Mutex<Database>,
    processor: DocumentProcessor,
    watch_folder: Mutex<Option<PathBuf>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    watch_folder: Option<String>,
    output_folder: Option<String>,
    ocr_enabled: bool,
    ai_classification: bool,
}

#[derive(Debug, Serialize)]
struct Stats {
    total: u64,
    processed: u64,
    failed: u64,
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Result<Settings, String> {
    let watch = state.watch_folder.lock().unwrap();
    Ok(Settings {
        watch_folder: watch.as_ref().map(|p| p.to_string_lossy().to_string()),
        output_folder: None,
        ocr_enabled: true,
        ai_classification: true,
    })
}

#[tauri::command]
fn set_watch_folder(state: State<AppState>, path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    if !path.exists() {
        return Err("Folder does not exist".to_string());
    }

    let mut watch = state.watch_folder.lock().unwrap();
    *watch = Some(path.clone());

    // Save to database
    let db = state.db.lock().unwrap();
    db.set_setting("watch_folder", &path.to_string_lossy())
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn get_stats(state: State<AppState>) -> Result<Stats, String> {
    let db = state.db.lock().unwrap();
    let stats = db.get_stats().map_err(|e| e.to_string())?;
    Ok(stats)
}

#[tauri::command]
async fn process_document(state: State<'_, AppState>, path: String) -> Result<ProcessedDocument, String> {
    let path = PathBuf::from(&path);

    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    let mut result = state.processor.process(&path).await.map_err(|e| e.to_string())?;

    // Save to database. On reprocess, the row is saved under the
    // existing document's id, not `result.id` (freshly generated for
    // this run) — overwrite it so the frontend gets an id that actually
    // resolves via get_document.
    let db = state.db.lock().unwrap();
    result.id = db.save_document(&result).map_err(|e| e.to_string())?;

    Ok(result)
}

#[tauri::command]
fn get_recent_documents(state: State<AppState>, limit: u32) -> Result<Vec<ProcessedDocument>, String> {
    let db = state.db.lock().unwrap();
    db.get_recent_documents(limit).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_document_details(state: State<AppState>, id: String) -> Result<ProcessedDocument, String> {
    let db = state.db.lock().unwrap();
    db.get_document(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_duplicates(state: State<AppState>) -> Result<usize, String> {
    let db = state.db.lock().unwrap();
    db.clear_duplicates().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_all_documents(state: State<AppState>) -> Result<usize, String> {
    let db = state.db.lock().unwrap();
    db.delete_all_documents().map_err(|e| e.to_string())
}

#[tauri::command]
fn update_document_type(state: State<AppState>, id: String, doc_type: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.update_document_type(&id, &doc_type).map_err(|e| e.to_string())
}

#[tauri::command]
async fn scan_folder(state: State<'_, AppState>, path: String) -> Result<Vec<ProcessedDocument>, String> {
    let folder = PathBuf::from(&path);
    if !folder.is_dir() {
        return Err("Not a valid directory".to_string());
    }

    let extensions = ["pdf", "docx", "doc", "txt"];
    let mut results = vec![];

    let entries: Vec<_> = std::fs::read_dir(&folder)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect();

    for entry in entries {
        let file_path = entry.path();
        let path_str = file_path.to_string_lossy().to_string();

        // Skip if already processed
        {
            let db = state.db.lock().unwrap();
            if db.document_exists(&path_str).unwrap_or(false) {
                continue;
            }
        }

        // Process document
        match state.processor.process(&file_path).await {
            Ok(mut doc) => {
                let db = state.db.lock().unwrap();
                if let Ok(saved_id) = db.save_document(&doc) {
                    doc.id = saved_id;
                    results.push(doc);
                }
            }
            Err(e) => {
                eprintln!("Failed to process {}: {}", file_path.display(), e);
            }
        }
    }

    Ok(results)
}

#[derive(Debug, Serialize)]
struct ExportedDocument {
    id: String,
    filename: String,
    doc_type: Option<String>,
    text: String,
    chunks: Vec<String>,
    metadata: ExportMetadata,
}

#[derive(Debug, Serialize)]
struct ExportMetadata {
    original_path: String,
    pages: Option<u32>,
    words: Option<u32>,
    size: u64,
    processed_at: String,
    classification_confidence: Option<f64>,
}

#[tauri::command]
async fn export_to_json(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.lock().unwrap();
    let docs = db.get_recent_documents(1000).map_err(|e| e.to_string())?;
    drop(db);

    // Get project root for export path
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();

    let project_root = exe_dir
        .ancestors()
        .find(|p| p.join("src-tauri").is_dir() && p.join("package.json").exists())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/opt/document-processor"));

    let export_dir = project_root.join("dane").join("export");
    std::fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;

    let mut manifest = vec![];

    for doc in &docs {
        // Get full document with text
        let db = state.db.lock().unwrap();
        let full_doc = db.get_document(&doc.id).map_err(|e| e.to_string())?;
        drop(db);

        let text = full_doc.full_text.unwrap_or_default();

        // Create chunks (simple: split by ~1000 chars at sentence boundaries)
        let chunks = create_chunks(&text, 1000);

        let exported = ExportedDocument {
            id: doc.id.clone(),
            filename: doc.filename.clone(),
            doc_type: doc.doc_type.clone(),
            text: text.clone(),
            chunks,
            metadata: ExportMetadata {
                original_path: doc.original_path.clone(),
                pages: doc.pages,
                words: doc.word_count,
                size: doc.size,
                processed_at: doc.processed_at.clone(),
                classification_confidence: doc.classification_confidence,
            },
        };

        // Save individual JSON
        let json_path = export_dir.join(format!("{}.json", doc.id));
        let json = serde_json::to_string_pretty(&exported).map_err(|e| e.to_string())?;
        std::fs::write(&json_path, &json).map_err(|e| e.to_string())?;

        manifest.push(serde_json::json!({
            "id": doc.id,
            "filename": doc.filename,
            "doc_type": doc.doc_type,
            "path": json_path.to_string_lossy(),
        }));
    }

    // Save manifest
    let manifest_path = export_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&serde_json::json!({
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "count": manifest.len(),
        "documents": manifest,
    })).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, &manifest_json).map_err(|e| e.to_string())?;

    Ok(export_dir.to_string_lossy().to_string())
}

#[tauri::command]
async fn open_file(path: String) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to open file: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn export_document_html(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let db = state.db.lock().unwrap();
    let doc = db.get_document(&id).map_err(|e| e.to_string())?;
    drop(db);

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();

    let project_root = exe_dir
        .ancestors()
        .find(|p| p.join("src-tauri").is_dir() && p.join("package.json").exists())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/opt/document-processor"));

    let export_dir = project_root.join("dane").join("export");
    std::fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;

    let html_content = format!(
        r#"<!DOCTYPE html>
<html lang="pl">
<head>
    <meta charset="UTF-8">
    <title>{}</title>
    <style>
        body {{ font-family: Arial, sans-serif; max-width: 800px; margin: 40px auto; padding: 20px; }}
        h1 {{ color: #333; border-bottom: 2px solid #4f46e5; padding-bottom: 10px; }}
        .meta {{ background: #f5f5f5; padding: 15px; border-radius: 8px; margin: 20px 0; display: flex; gap: 30px; }}
        .meta-item {{ }}
        .meta-item strong {{ color: #666; }}
        .content {{ white-space: pre-wrap; line-height: 1.8; font-size: 14px; }}
        @media print {{ body {{ margin: 20px; }} }}
    </style>
</head>
<body>
    <h1>{}</h1>
    <div class="meta">
        <div class="meta-item"><strong>Typ:</strong> {}</div>
        <div class="meta-item"><strong>Strony:</strong> {}</div>
        <div class="meta-item"><strong>Słowa:</strong> {}</div>
        <div class="meta-item"><strong>Rozmiar:</strong> {} KB</div>
    </div>
    <div class="content">{}</div>
    <script>window.onload = function() {{ window.print(); }}</script>
</body>
</html>"#,
        doc.filename,
        doc.filename,
        doc.doc_type.as_deref().unwrap_or("nieznany"),
        doc.pages.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string()),
        doc.word_count.map(|w| w.to_string()).unwrap_or_else(|| "N/A".to_string()),
        doc.size / 1024,
        doc.full_text.as_deref().unwrap_or("Brak treści").replace('<', "&lt;").replace('>', "&gt;")
    );

    let safe_filename = doc.filename.replace(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_', "_");
    let html_path = export_dir.join(format!("{}_print.html", safe_filename));
    std::fs::write(&html_path, &html_content).map_err(|e| e.to_string())?;

    Ok(html_path.to_string_lossy().to_string())
}

#[tauri::command]
async fn export_document_md(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let db = state.db.lock().unwrap();
    let doc = db.get_document(&id).map_err(|e| e.to_string())?;
    drop(db);

    // Get project root for export path
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();

    let project_root = exe_dir
        .ancestors()
        .find(|p| p.join("src-tauri").is_dir() && p.join("package.json").exists())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/opt/document-processor"));

    let export_dir = project_root.join("dane").join("export");
    std::fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;

    // Create markdown content
    let md_content = format!(
        r#"# {}

## Metadane

| Pole | Wartość |
|------|---------|
| Typ dokumentu | {} |
| Ścieżka | {} |
| Strony | {} |
| Słowa | {} |
| Rozmiar | {} bajtów |
| Przetworzono | {} |

## Treść dokumentu

{}
"#,
        doc.filename,
        doc.doc_type.as_deref().unwrap_or("nieznany"),
        doc.original_path,
        doc.pages.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string()),
        doc.word_count.map(|w| w.to_string()).unwrap_or_else(|| "N/A".to_string()),
        doc.size,
        doc.processed_at,
        doc.full_text.as_deref().unwrap_or("Brak treści")
    );

    // Save markdown file
    let safe_filename = doc.filename.replace(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_', "_");
    let md_path = export_dir.join(format!("{}.md", safe_filename));
    std::fs::write(&md_path, &md_content).map_err(|e| e.to_string())?;

    Ok(md_path.to_string_lossy().to_string())
}

fn create_chunks(text: &str, target_size: usize) -> Vec<String> {
    let mut chunks = vec![];
    let mut current = String::new();

    for sentence in text.split(|c| c == '.' || c == '\n') {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }

        if current.len() + sentence.len() > target_size && !current.is_empty() {
            chunks.push(current.clone());
            current.clear();
        }

        if !current.is_empty() {
            current.push_str(". ");
        }
        current.push_str(sentence);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

#[tauri::command]
async fn scan_folder_force(state: State<'_, AppState>, path: String) -> Result<Vec<ProcessedDocument>, String> {
    let folder = PathBuf::from(&path);
    if !folder.is_dir() {
        return Err("Not a valid directory".to_string());
    }

    let extensions = ["pdf", "docx", "doc", "txt"];
    let mut results = vec![];

    let entries: Vec<_> = std::fs::read_dir(&folder)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect();

    for entry in entries {
        let file_path = entry.path();

        // Force process (update existing)
        match state.processor.process(&file_path).await {
            Ok(mut doc) => {
                let db = state.db.lock().unwrap();
                if let Ok(saved_id) = db.save_document(&doc) {
                    doc.id = saved_id;
                    results.push(doc);
                }
            }
            Err(e) => {
                eprintln!("Failed to process {}: {}", file_path.display(), e);
            }
        }
    }

    Ok(results)
}

fn main() {
    // Initialize database in project's data folder
    // Get the executable's directory or use current dir
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // Use data/ folder in project root (go up from target/release)
    // Look for directory that has src-tauri as child (not as self name)
    let project_root = exe_dir
        .ancestors()
        .find(|p| p.join("src-tauri").is_dir() && p.join("package.json").exists())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| {
            // Fallback: hardcoded project path for development
            PathBuf::from("/opt/document-processor")
        });

    let data_dir = project_root.join("dane");
    let db_path = data_dir.join("documents.db");

    std::fs::create_dir_all(&data_dir).ok();
    std::fs::create_dir_all(project_root.join("sekrety")).ok();

    let db = Database::new(&db_path).expect("Failed to initialize database");

    // Load watch folder from settings
    let watch_folder = db.get_setting("watch_folder").ok().flatten().map(PathBuf::from);

    println!("Document Processor data directory: {}", data_dir.display());

    let app_state = AppState {
        db: Mutex::new(db),
        processor: DocumentProcessor::new(data_dir),
        watch_folder: Mutex::new(watch_folder),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_watch_folder,
            get_stats,
            process_document,
            get_recent_documents,
            get_document_details,
            clear_duplicates,
            delete_all_documents,
            update_document_type,
            scan_folder,
            scan_folder_force,
            export_to_json,
            export_document_md,
            export_document_html,
            open_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
