//! Hybrid search: SQLite FTS5 (lexical, with exact-fragment highlighting)
//! combined with embedding cosine similarity (semantic, via a local
//! Ollama instance) using Reciprocal Rank Fusion (RRF).
//!
//! Pattern follows `core/search/` in the (separate, Python) Legal
//! Assistant v5 project — full-text + semantic + hybrid reranking with
//! offset-based source highlighting — rewritten natively in Rust for
//! this codebase rather than integrated cross-repo (this app has no
//! Python runtime, and coupling to another repo's module isn't
//! appropriate here).
//!
//! Local-first: embeddings come from a local Ollama instance
//! (`nomic-embed-text`, 768-dim). No embedding, no cloud call — if
//! Ollama is unreachable, semantic search silently degrades to
//! lexical-only (see `embed_text`'s Result, callers treat Err as "skip
//! the semantic half", not a failure of the whole search).

use serde::{Deserialize, Serialize};

/// A chunk ready to be persisted by `Database::replace_chunks` — text +
/// its exact offsets into the source document, plus its embedding if
/// Ollama was reachable when it was computed (`None` otherwise; the row
/// is still stored so lexical search still finds it).
pub struct IndexedChunk {
    pub text: String,
    pub start: i64,
    pub end: i64,
    pub embedding: Option<Vec<f32>>,
}

pub const EMBED_DIM: usize = 768;
const OLLAMA_EMBED_URL: &str = "http://localhost:11434/api/embed";
const OLLAMA_MODEL: &str = "nomic-embed-text";
/// Observed empirically (2026-07-07): a batch-indexing run against a
/// real local Ollama instance stalled on one chunk — CPU in the
/// llama-server subprocess kept climbing with no response for over six
/// minutes, on hardware that embeds a typical ~1000-byte chunk in well
/// under a second otherwise. Root cause not isolated (a specific
/// chunk's content, model/context-window interaction, or a resource
/// contention issue), but with no timeout at all a single such request
/// blocks the entire sequential indexing loop indefinitely — every
/// chunk after it in every document queued behind it never gets
/// embedded. 30s is generous versus normal latency (<1s) while still
/// bounding the worst case; embed_text's fail-open contract (Err →
/// caller stores embedding: None, lexical search still finds the
/// chunk) means a timeout here degrades one chunk's semantic
/// searchability, not the app.
const EMBED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// RRF constant. Standard choice (Cormack et al.) — large enough that a
/// single ranking list dominating rank 1 doesn't swamp the other list's
/// contribution entirely, small enough that rank position still matters.
const RRF_K: f64 = 60.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub document_id: String,
    pub filename: String,
    pub doc_type: Option<String>,
    /// The best-matching chunk's text, exactly as it appears in the
    /// source document (not reconstructed/rejoined).
    pub snippet: String,
    /// Byte offsets of `snippet` within the document's full_text —
    /// what a "scroll to this fragment and highlight it" UI needs.
    pub start_offset: i64,
    pub end_offset: i64,
    /// Pre-split segments for lexical hits (`None` for semantic-only
    /// hits — no literal term overlap to mark). Segments, not a raw
    /// HTML string: `snippet` is exact document content, not something
    /// safe to inject via `{@html}` on the frontend (a PDF/DOCX could
    /// contain literal "<"/">" text). The frontend renders `text`
    /// through normal auto-escaped interpolation and only wraps
    /// `marked` segments in `<mark>` itself — no raw HTML crosses IPC.
    pub highlighted: Option<Vec<HighlightSegment>>,
    pub score: f64,
    /// Which signal(s) produced this hit — surfaced so the UI can show
    /// e.g. a small "semantic match" badge distinctly from a lexical one.
    pub matched_lexical: bool,
    pub matched_semantic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HighlightSegment {
    pub text: String,
    pub marked: bool,
}

/// Split FTS5 `highlight()` output — delimited with U+0001/U+0002
/// control-character markers instead of literal HTML tags (see
/// `search_lexical_ranked` in db.rs) — into safe-to-render segments.
pub fn parse_highlight_markers(s: &str) -> Vec<HighlightSegment> {
    let mut segments = Vec::new();
    let mut marked = false;
    let mut current = String::new();
    for c in s.chars() {
        match c {
            '\u{1}' => {
                if !current.is_empty() {
                    segments.push(HighlightSegment { text: std::mem::take(&mut current), marked });
                }
                marked = true;
            }
            '\u{2}' => {
                if !current.is_empty() {
                    segments.push(HighlightSegment { text: std::mem::take(&mut current), marked });
                }
                marked = false;
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        segments.push(HighlightSegment { text: current, marked });
    }
    segments
}

/// Split `text` into chunks that are exact substrings of the original
/// (byte offsets included), breaking on sentence-ish boundaries ('.'
/// or '\n') once a chunk reaches `target_size` bytes. Unlike the
/// existing `create_chunks` in main.rs (used for JSON export), this
/// does not rejoin sentences with a synthesized separator — every
/// chunk's (start, end) is a real slice of `text`, required for
/// highlighting/jump-to-fragment to point at real document positions.
pub fn chunk_with_offsets(text: &str, target_size: usize) -> Vec<(String, usize, usize)> {
    let mut chunks = Vec::new();
    let mut chunk_start = 0usize;
    let bytes = text.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        if b == b'.' || b == b'\n' {
            let boundary_end = i + 1;
            if boundary_end - chunk_start >= target_size {
                push_trimmed(&mut chunks, text, chunk_start, boundary_end);
                chunk_start = boundary_end;
            }
        }
    }
    if chunk_start < text.len() {
        push_trimmed(&mut chunks, text, chunk_start, text.len());
    }
    chunks
}

fn push_trimmed(chunks: &mut Vec<(String, usize, usize)>, text: &str, start: usize, end: usize) {
    let slice = &text[start..end];
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return;
    }
    // '.' and '\n' are single-byte ASCII, so `start`/`end` are already
    // valid UTF-8 boundaries; trim_start()'s byte delta preserves that.
    let leading_ws = slice.len() - slice.trim_start().len();
    let trimmed_start = start + leading_ws;
    let trimmed_end = trimmed_start + trimmed.len();
    chunks.push((trimmed.to_string(), trimmed_start, trimmed_end));
}

/// Serialize an embedding vector to bytes for BLOB storage (4 bytes/f32,
/// little-endian). No vector-search extension needed at this scale — a
/// local desktop app's corpus is realistically hundreds to low
/// thousands of chunks, and cosine similarity over that many 768-dim
/// vectors in Rust is sub-millisecond.
pub fn embedding_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

pub fn bytes_to_embedding(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

/// Embed one string via the local Ollama instance. `Err` means "Ollama
/// unreachable or the model isn't pulled" — callers degrade to
/// lexical-only search rather than failing outright; this must never be
/// the reason an import fails (see db.rs::index_document_chunks).
pub async fn embed_text(text: &str) -> Result<Vec<f32>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(OLLAMA_EMBED_URL)
        .timeout(EMBED_TIMEOUT)
        .json(&serde_json::json!({ "model": OLLAMA_MODEL, "input": text }))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("ollama request timed out after {}s", EMBED_TIMEOUT.as_secs())
            } else {
                format!("ollama unreachable: {e}")
            }
        })?;

    if !resp.status().is_success() {
        return Err(format!("ollama returned {}", resp.status()));
    }

    let parsed: OllamaEmbedResponse = resp
        .json()
        .await
        .map_err(|e| format!("bad ollama response: {e}"))?;

    let vector = parsed
        .embeddings
        .into_iter()
        .next()
        .ok_or_else(|| "ollama returned no embedding".to_string())?;

    // Catches a silently-swapped model (e.g. someone changes OLLAMA_MODEL
    // to one with a different embedding_length) before it corrupts
    // cosine_similarity results by comparing vectors of different
    // dimensionality against each other.
    if vector.len() != EMBED_DIM {
        return Err(format!(
            "embedding dimension mismatch: expected {EMBED_DIM}, got {}",
            vector.len()
        ));
    }

    Ok(vector)
}

/// Reciprocal Rank Fusion over two ranked id lists. Avoids having to
/// calibrate BM25 (unbounded, corpus-dependent) against cosine
/// similarity (bounded -1..1) on a shared scale — RRF only cares about
/// rank position within each list, not the raw scores.
pub fn reciprocal_rank_fusion(
    lexical_ranked: &[String],
    semantic_ranked: &[String],
) -> Vec<(String, f64, bool, bool)> {
    use std::collections::HashMap;

    let mut scores: HashMap<String, (f64, bool, bool)> = HashMap::new();

    for (rank, id) in lexical_ranked.iter().enumerate() {
        let entry = scores.entry(id.clone()).or_insert((0.0, false, false));
        entry.0 += 1.0 / (RRF_K + rank as f64 + 1.0);
        entry.1 = true;
    }
    for (rank, id) in semantic_ranked.iter().enumerate() {
        let entry = scores.entry(id.clone()).or_insert((0.0, false, false));
        entry.0 += 1.0 / (RRF_K + rank as f64 + 1.0);
        entry.2 = true;
    }

    let mut out: Vec<(String, f64, bool, bool)> =
        scores.into_iter().map(|(id, (s, l, sem))| (id, s, l, sem)).collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_highlight_markers_splits_marked_and_unmarked_segments() {
        let raw = "To jest \u{1}najmu\u{2} lokalu";
        let segments = parse_highlight_markers(raw);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0], HighlightSegment { text: "To jest ".into(), marked: false });
        assert_eq!(segments[1], HighlightSegment { text: "najmu".into(), marked: true });
        assert_eq!(segments[2], HighlightSegment { text: " lokalu".into(), marked: false });
    }

    #[test]
    fn parse_highlight_markers_handles_no_markers() {
        let segments = parse_highlight_markers("zwykly tekst bez trafien");
        assert_eq!(segments.len(), 1);
        assert!(!segments[0].marked);
    }

    #[test]
    fn parse_highlight_markers_never_leaks_the_raw_control_characters() {
        let raw = "\u{1}kill\u{2} -9 process, \u{1}sudo\u{2} rm";
        let segments = parse_highlight_markers(raw);
        for seg in &segments {
            assert!(!seg.text.contains('\u{1}'));
            assert!(!seg.text.contains('\u{2}'));
        }
    }

    #[test]
    fn chunks_are_exact_substrings_with_correct_offsets() {
        let text = "Pierwsze zdanie. Drugie zdanie tez dosc dlugie tutaj. Trzecie.";
        let chunks = chunk_with_offsets(text, 20);
        for (chunk_text, start, end) in &chunks {
            assert_eq!(&text[*start..*end], chunk_text.as_str());
        }
        assert!(chunks.len() >= 2, "long text should split into >=2 chunks, got {}", chunks.len());
    }

    #[test]
    fn chunk_offsets_survive_polish_diacritics() {
        let text = "Zażółć gęślą jaźń. Kolejne zdanie z polskimi znakami ąęćłńóśźż.";
        let chunks = chunk_with_offsets(text, 10);
        for (chunk_text, start, end) in &chunks {
            assert_eq!(&text[*start..*end], chunk_text.as_str());
        }
    }

    #[test]
    fn short_text_is_a_single_chunk() {
        let chunks = chunk_with_offsets("Krotki tekst.", 1000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, "Krotki tekst.");
    }

    #[test]
    fn empty_text_produces_no_chunks() {
        assert_eq!(chunk_with_offsets("", 100).len(), 0);
        assert_eq!(chunk_with_offsets("   \n  ", 100).len(), 0);
    }

    #[test]
    fn embedding_bytes_roundtrip() {
        let v: Vec<f32> = vec![0.1, -0.2, 3.14159, 0.0, -1.0];
        let bytes = embedding_to_bytes(&v);
        let back = bytes_to_embedding(&bytes);
        assert_eq!(v.len(), back.len());
        for (a, b) in v.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn cosine_similarity_identical_vectors_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6, "got {sim}");
    }

    #[test]
    fn cosine_similarity_orthogonal_vectors_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_opposite_vectors_is_negative_one() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 1e-6, "got {sim}");
    }

    #[test]
    fn rrf_favours_ids_ranked_well_in_both_lists() {
        let lexical = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let semantic = vec!["b".to_string(), "a".to_string(), "d".to_string()];
        let fused = reciprocal_rank_fusion(&lexical, &semantic);
        // "a" and "b" both appear near the top of both lists; "c"/"d" each
        // appear once, lower. Top two fused results must be {a, b}.
        let top_two: std::collections::HashSet<_> =
            fused.iter().take(2).map(|(id, ..)| id.clone()).collect();
        assert!(top_two.contains("a"));
        assert!(top_two.contains("b"));
    }

    #[test]
    fn rrf_marks_which_list_each_id_came_from() {
        let lexical = vec!["a".to_string()];
        let semantic = vec!["b".to_string()];
        let fused = reciprocal_rank_fusion(&lexical, &semantic);
        let a = fused.iter().find(|(id, ..)| id == "a").unwrap();
        assert!(a.2 && !a.3, "a: lexical=true, semantic=false expected");
        let b = fused.iter().find(|(id, ..)| id == "b").unwrap();
        assert!(!b.2 && b.3, "b: lexical=false, semantic=true expected");
    }

    #[test]
    fn rrf_id_in_both_lists_scores_higher_than_id_in_one() {
        let lexical = vec!["shared".to_string(), "only_lexical".to_string()];
        let semantic = vec!["shared".to_string(), "only_semantic".to_string()];
        let fused = reciprocal_rank_fusion(&lexical, &semantic);
        let shared_score = fused.iter().find(|(id, ..)| id == "shared").unwrap().1;
        let lexical_only_score = fused.iter().find(|(id, ..)| id == "only_lexical").unwrap().1;
        assert!(shared_score > lexical_only_score);
    }
}
