//! Journal ingestion helpers.
//!
//! This ports the Python app's markdown + embeddings loading into a small,
//! deterministic scanner. LanceDB table mutation lives in `lance.rs`; this
//! module only turns configured files into normalized journal rows.

use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use super::{DbError, DbResult};

#[derive(Debug, Clone)]
pub struct JournalRow {
    pub entry_id: String,
    pub date: String,
    pub title: String,
    pub text: String,
    pub tags: Vec<String>,
    pub embedding: Vec<f32>,
    pub entry_type: String,
    pub source_path: String,
    pub embedding_key: String,
    pub content_hash: String,
    pub source_mtime_ms: i64,
    pub indexed_at: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingLine {
    path: String,
    embedding: Vec<f32>,
}

#[derive(Debug, Default)]
struct EmbeddingMap {
    by_exact_path: HashMap<String, Vec<f32>>,
    by_file_stem: HashMap<String, Vec<f32>>,
    by_evergreen_rel: HashMap<String, Vec<f32>>,
}

pub fn load_journal_rows(
    journal_dir: Option<&Path>,
    evergreen_dir: Option<&Path>,
    embeddings_path: &Path,
) -> DbResult<Vec<JournalRow>> {
    let embeddings = load_embeddings(embeddings_path)?;
    let mut rows = Vec::new();

    if let Some(dir) = journal_dir {
        if dir.exists() {
            rows.extend(load_daily_rows(dir, &embeddings)?);
        } else {
            eprintln!(
                "[lancedb] journal dir does not exist, skipping: {}",
                dir.display()
            );
        }
    }

    if let Some(dir) = evergreen_dir {
        if dir.exists() {
            rows.extend(load_evergreen_rows(dir, &embeddings)?);
        } else {
            eprintln!(
                "[lancedb] evergreen dir does not exist, skipping: {}",
                dir.display()
            );
        }
    }

    Ok(rows)
}

fn load_embeddings(path: &Path) -> DbResult<EmbeddingMap> {
    let mut out = EmbeddingMap::default();
    let content = fs::read_to_string(path)?;

    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: EmbeddingLine = serde_json::from_str(line).map_err(|err| {
            DbError::InvalidData(format!(
                "invalid embeddings jsonl at line {} in {}: {err}",
                idx + 1,
                path.display()
            ))
        })?;

        if parsed.embedding.is_empty() {
            continue;
        }

        out.by_exact_path
            .insert(parsed.path.clone(), parsed.embedding.clone());
        if let Some(stem) = Path::new(&parsed.path).file_stem().and_then(|s| s.to_str()) {
            out.by_file_stem
                .insert(stem.to_string(), parsed.embedding.clone());
        }
        if let Some(rel) = path_after_component(&parsed.path, "Evergreen") {
            out.by_evergreen_rel.insert(rel, parsed.embedding);
        }
    }

    Ok(out)
}

fn load_daily_rows(dir: &Path, embeddings: &EmbeddingMap) -> DbResult<Vec<JournalRow>> {
    let mut rows = Vec::new();
    for path in markdown_files(dir)? {
        let Some(title) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };

        if title.ends_with("- Week") {
            continue;
        }

        let dates = match parse_daily_dates(&title) {
            Ok(dates) => dates,
            Err(_) => {
                eprintln!("[lancedb] skipping daily file with invalid date format: {title}");
                continue;
            }
        };

        let Some(embedding) = embeddings.by_file_stem.get(&title).cloned() else {
            eprintln!("[lancedb] no embedding for daily entry: {title}");
            continue;
        };

        let content = fs::read_to_string(&path)?;
        let text = extract_transcription(&content);
        let tags = extract_tags(&content);
        let source_path = path_to_string(&path);
        let source_mtime_ms = source_mtime_ms(&path)?;
        let content_hash = stable_content_hash(&[
            &source_path,
            &title,
            &text,
            &tags.join("\u{1f}"),
            &embedding_hash(&embedding),
        ]);

        for date in dates {
            let date = date.format("%Y-%m-%d").to_string();
            rows.push(JournalRow {
                entry_id: format!("daily:{source_path}:{date}"),
                date,
                title: title.clone(),
                text: text.clone(),
                tags: tags.clone(),
                embedding: embedding.clone(),
                entry_type: "daily".to_string(),
                source_path: source_path.clone(),
                embedding_key: title.clone(),
                content_hash: content_hash.clone(),
                source_mtime_ms,
                indexed_at: Utc::now().to_rfc3339(),
            });
        }
    }
    Ok(rows)
}

fn load_evergreen_rows(dir: &Path, embeddings: &EmbeddingMap) -> DbResult<Vec<JournalRow>> {
    let mut rows = Vec::new();
    for path in markdown_files(dir)? {
        let source_path = path_to_string(&path);
        let canonical_path = fs::canonicalize(&path).ok().map(|p| path_to_string(&p));
        let evergreen_rel = path
            .strip_prefix(dir)
            .ok()
            .map(normalized_path_string)
            .or_else(|| path_after_component(&source_path, "Evergreen"));
        let embedding = embeddings
            .by_exact_path
            .get(&source_path)
            .or_else(|| {
                canonical_path
                    .as_ref()
                    .and_then(|p| embeddings.by_exact_path.get(p))
            })
            .or_else(|| {
                evergreen_rel
                    .as_ref()
                    .and_then(|rel| embeddings.by_evergreen_rel.get(rel))
            })
            .cloned();

        let Some(embedding) = embedding else {
            eprintln!("[lancedb] no embedding for evergreen entry: {source_path}");
            continue;
        };

        let content = fs::read_to_string(&path)?;
        let text = strip_frontmatter(&content).trim().to_string();
        if text.is_empty() {
            continue;
        }

        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        let tags = extract_tags(&content);
        let source_mtime_ms = source_mtime_ms(&path)?;
        let date = DateTime::<Utc>::from_timestamp_millis(source_mtime_ms)
            .unwrap_or_else(Utc::now)
            .format("%Y-%m-%d")
            .to_string();
        let embedding_key = canonical_path.unwrap_or_else(|| source_path.clone());
        let content_hash = stable_content_hash(&[
            &source_path,
            &title,
            &text,
            &tags.join("\u{1f}"),
            &embedding_hash(&embedding),
        ]);

        rows.push(JournalRow {
            entry_id: format!("evergreen:{source_path}"),
            date,
            title,
            text,
            tags,
            embedding,
            entry_type: "evergreen".to_string(),
            source_path,
            embedding_key,
            content_hash,
            source_mtime_ms,
            indexed_at: Utc::now().to_rfc3339(),
        });
    }
    Ok(rows)
}

fn markdown_files(root: &Path) -> DbResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_markdown_files(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_markdown_files(path: &Path, out: &mut Vec<PathBuf>) -> DbResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_daily_dates(title: &str) -> Result<Vec<NaiveDate>, chrono::ParseError> {
    title
        .split('_')
        .map(|part| NaiveDate::parse_from_str(part, "%m-%d-%Y"))
        .collect()
}

fn extract_transcription(text: &str) -> String {
    let Some(start) = text.find("### Transcription") else {
        return String::new();
    };
    let after_header = &text[start + "### Transcription".len()..];
    let end = after_header
        .lines()
        .scan(0usize, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(|(offset, line)| {
            if line.trim_start().starts_with("###") {
                Some(offset)
            } else {
                None
            }
        })
        .unwrap_or(after_header.len());
    after_header[..end].trim().to_string()
}

fn extract_tags(text: &str) -> Vec<String> {
    let mut tags = HashSet::new();
    for token in text.split(|c: char| {
        !(c.is_alphanumeric() || c == '_' || c == '-' || c == '/' || c == '#')
    }) {
        if token.len() > 1 && token.starts_with('#') {
            let mut chars = token.chars();
            let _hash = chars.next();
            if !chars
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let tag = token
                .chars()
                .take_while(|c| {
                    *c == '#'
                        || c.is_alphanumeric()
                        || *c == '_'
                        || *c == '-'
                        || *c == '/'
                })
                .collect::<String>();
            if tag.len() > 1 {
                tags.insert(tag);
            }
        }
    }
    let mut tags = tags.into_iter().collect::<Vec<_>>();
    tags.sort();
    tags
}

fn strip_frontmatter(text: &str) -> String {
    if !text.starts_with("---") {
        return text.to_string();
    }
    let mut lines = text.lines();
    let _ = lines.next();
    let mut consumed = 4usize; // opening --- plus newline-ish; only used for slicing fallback below
    for line in lines {
        consumed += line.len() + 1;
        if line.trim() == "---" {
            return text.get(consumed..).unwrap_or_default().to_string();
        }
    }
    text.to_string()
}

fn source_mtime_ms(path: &Path) -> DbResult<i64> {
    let modified = fs::metadata(path)?.modified()?;
    let duration = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| DbError::InvalidData(format!("mtime before unix epoch: {err}")))?;
    Ok(duration.as_millis() as i64)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn normalized_path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

fn path_after_component(path: &str, component: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let needle = format!("/{component}/");
    normalized
        .find(&needle)
        .map(|idx| normalized[idx + needle.len()..].to_string())
        .or_else(|| {
            normalized
                .strip_prefix(&format!("{component}/"))
                .map(str::to_string)
        })
}

fn stable_content_hash(parts: &[&str]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
        "\u{1e}".hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn embedding_hash(embedding: &[f32]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for value in embedding {
        value.to_bits().hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_transcription_until_next_header() {
        let text = "# x\n### Transcription\nhello\nworld\n### Other\nnope";
        assert_eq!(extract_transcription(text), "hello\nworld");
    }

    #[test]
    fn extracts_unique_sorted_tags() {
        assert_eq!(
            extract_tags("hi #z #a #a and #tag_1 #Work/EK #mental-health"),
            vec!["#Work/EK", "#a", "#mental-health", "#tag_1", "#z"]
        );
    }

    #[test]
    fn extracts_evergreen_relative_path_from_unix_or_wsl_paths() {
        assert_eq!(
            path_after_component(
                "/mnt/c/Users/Administrator/OneDrive/Journal/Evergreen/Plans/Home/Sunroom.md",
                "Evergreen"
            ),
            Some("Plans/Home/Sunroom.md".to_string())
        );
        assert_eq!(
            path_after_component(
                "/Users/hamiltones/OneDrive/Journal/Evergreen/neurostack/Projects/Projects.md",
                "Evergreen"
            ),
            Some("neurostack/Projects/Projects.md".to_string())
        );
    }
}
