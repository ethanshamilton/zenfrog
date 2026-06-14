use std::{collections::{BTreeSet, HashMap}, fs, path::{Path, PathBuf}, process::Command};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{ai, db::{DbError, DbResult}};

const PAGE_TEMPLATE_PREFIX: &str = "#day\n### Page\n";

#[derive(Debug, Clone)]
pub struct IngestionConfig {
    pub journal_dir: Option<PathBuf>,
    pub evergreen_dir: Option<PathBuf>,
    pub embeddings_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct TranscriptionJob {
    source_path: PathBuf,
    markdown_path: PathBuf,
}

#[derive(Debug, Default)]
struct DailyCrawlResult {
    to_transcribe: Vec<TranscriptionJob>,
    to_embed: Vec<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize)]
struct EmbeddingLine {
    path: String,
    embedding: Vec<f64>,
}

pub async fn run_startup_pipeline(config: &IngestionConfig) -> DbResult<()> {
    let Some(embeddings_path) = config.embeddings_path.as_deref() else {
        eprintln!("[ingestion] ZENFROG_EMBEDDINGS_PATH unset; skipping transcription/embedding pipeline");
        return Ok(());
    };

    if let Some(parent) = embeddings_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !embeddings_path.exists() {
        fs::write(embeddings_path, "")?;
    }

    let tags = collect_tags(config.journal_dir.as_deref(), config.evergreen_dir.as_deref())?;

    if let Some(journal_dir) = config.journal_dir.as_deref() {
        if journal_dir.exists() {
            let daily = crawl_daily_entries(journal_dir)?;
            eprintln!(
                "[ingestion] daily crawl: {} to transcribe, {} to embed",
                daily.to_transcribe.len(),
                daily.to_embed.len()
            );
            transcribe_daily_docs(&daily.to_transcribe, &tags).await?;
            embed_daily_docs(&daily.to_embed, embeddings_path).await?;
        } else {
            eprintln!("[ingestion] journal dir does not exist, skipping: {}", journal_dir.display());
        }
    }

    if let Some(evergreen_dir) = config.evergreen_dir.as_deref() {
        if evergreen_dir.exists() {
            let evergreen = crawl_evergreen_entries(evergreen_dir)?;
            eprintln!("[ingestion] evergreen crawl: {} to embed", evergreen.len());
            embed_evergreen_docs(&evergreen, embeddings_path).await?;
        } else {
            eprintln!("[ingestion] evergreen dir does not exist, skipping: {}", evergreen_dir.display());
        }
    }

    Ok(())
}

fn crawl_daily_entries(root: &Path) -> DbResult<DailyCrawlResult> {
    let mut result = DailyCrawlResult::default();
    for path in recursive_files(root)? {
        if !is_journal_source(&path) {
            continue;
        }
        let Some(md_path) = markdown_path_for_source(&path) else {
            continue;
        };
        if !md_path.exists() {
            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
            fs::write(&md_path, format!("{PAGE_TEMPLATE_PREFIX}![[{filename}]]\n"))?;
            eprintln!("[ingestion] created markdown stub: {}", md_path.display());
        }

        let content = fs::read_to_string(&md_path).unwrap_or_default();
        let frontmatter = parse_frontmatter(&content);
        if !frontmatter_is_true(&frontmatter, "transcription") {
            result.to_transcribe.push(TranscriptionJob {
                source_path: path.clone(),
                markdown_path: md_path.clone(),
            });
        }
        if !frontmatter_is_true(&frontmatter, "embedding") {
            result.to_embed.push(md_path);
        }
    }
    result.to_transcribe.sort_by(|a, b| a.markdown_path.cmp(&b.markdown_path));
    result.to_embed.sort();
    result.to_embed.dedup();
    Ok(result)
}

async fn transcribe_daily_docs(jobs: &[TranscriptionJob], tags: &str) -> DbResult<()> {
    for (idx, job) in jobs.iter().enumerate() {
        eprintln!(
            "[ingestion] transcribing {}/{}: {}",
            idx + 1,
            jobs.len(),
            job.source_path.display()
        );
        let images = encode_entry(&job.source_path)?;
        let transcription = ai::transcribe_image_data_urls(&images, tags)
            .await
            .map_err(DbError::InvalidData)?;
        insert_transcription(&job.markdown_path, &transcription)?;
    }
    Ok(())
}

async fn embed_daily_docs(files: &[PathBuf], embeddings_path: &Path) -> DbResult<()> {
    for (idx, file) in files.iter().enumerate() {
        eprintln!("[ingestion] embedding daily {}/{}: {}", idx + 1, files.len(), file.display());
        let content = fs::read_to_string(file)?;
        let transcription = extract_transcription(&content);
        if transcription.trim().is_empty() {
            eprintln!("[ingestion] skipping daily embedding with empty transcription: {}", file.display());
            continue;
        }
        let embedding = ai::embed_text(&transcription).await.map_err(DbError::InvalidData)?;
        upsert_embedding(embeddings_path, &canonical_or_original(file), embedding)?;
        update_frontmatter_field(file, "embedding", "True")?;
    }
    Ok(())
}

fn crawl_evergreen_entries(root: &Path) -> DbResult<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    for path in recursive_files(root)? {
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let body = strip_frontmatter(&content).trim().to_string();
        if body.is_empty() {
            continue;
        }
        let hash = compute_content_hash(&body);
        let frontmatter = parse_frontmatter(&content);
        let existing = frontmatter
            .get("content_hash")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if existing != hash {
            out.push((path, hash));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

async fn embed_evergreen_docs(files: &[(PathBuf, String)], embeddings_path: &Path) -> DbResult<()> {
    for (idx, (file, hash)) in files.iter().enumerate() {
        eprintln!("[ingestion] embedding evergreen {}/{}: {}", idx + 1, files.len(), file.display());
        let content = fs::read_to_string(file)?;
        let body = strip_frontmatter(&content).trim().to_string();
        if body.is_empty() {
            continue;
        }
        let embedding = ai::embed_text(&body).await.map_err(DbError::InvalidData)?;
        upsert_embedding(embeddings_path, &canonical_or_original(file), embedding)?;
        update_frontmatter_field(file, "embedding", "True")?;
        update_frontmatter_field(file, "content_hash", hash)?;
    }
    Ok(())
}

fn recursive_files(root: &Path) -> DbResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_files(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> DbResult<()> {
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn is_journal_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("pdf" | "png" | "jpg" | "jpeg")
    )
}

fn markdown_path_for_source(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let date_part = stem.split_whitespace().next()?;
    Some(path.parent()?.join(format!("{date_part}.md")))
}

fn parse_frontmatter(content: &str) -> HashMap<String, serde_yaml::Value> {
    if !content.starts_with("---") {
        return HashMap::new();
    }
    let Some(end) = content[3..].find("---") else {
        return HashMap::new();
    };
    serde_yaml::from_str(&content[3..3 + end]).unwrap_or_default()
}

fn frontmatter_is_true(frontmatter: &HashMap<String, serde_yaml::Value>, key: &str) -> bool {
    frontmatter
        .get(key)
        .and_then(|v| v.as_str())
        .map(|v| v == "True")
        .unwrap_or(false)
}

fn strip_frontmatter(content: &str) -> &str {
    if !content.starts_with("---") {
        return content;
    }
    let Some(end) = content[3..].find("---") else {
        return content;
    };
    content[3 + end + 3..].trim_start_matches('\n')
}

fn update_frontmatter_field(path: &Path, field: &str, value: &str) -> DbResult<()> {
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut frontmatter = parse_frontmatter(&content);
    let body = strip_frontmatter(&content);
    frontmatter.insert(field.to_string(), serde_yaml::Value::String(value.to_string()));
    let yaml = serde_yaml::to_string(&frontmatter)
        .map_err(|err| DbError::InvalidData(err.to_string()))?;
    fs::write(path, format!("---\n{yaml}---\n{body}"))?;
    Ok(())
}

fn compute_content_hash(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    format!("{:x}", hasher.finalize())
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
        .find_map(|(offset, line)| line.trim_start().starts_with("###").then_some(offset))
        .unwrap_or(after_header.len());
    after_header[..end].trim().to_string()
}

fn insert_transcription(path: &Path, transcription: &str) -> DbResult<()> {
    update_frontmatter_field(path, "transcription", "True")?;
    let content = fs::read_to_string(path)?;
    let body_start = frontmatter_body_start(&content);
    let (prefix, body) = content.split_at(body_start);
    let header = "### Transcription";

    let new_body = if let Some(start) = body.find(header) {
        let after_start = start + header.len();
        let after = &body[after_start..];
        let end = after
            .lines()
            .scan(0usize, |offset, line| {
                let current = *offset;
                *offset += line.len() + 1;
                Some((current, line))
            })
            .find_map(|(offset, line)| line.trim_start().starts_with('#').then_some(offset))
            .unwrap_or(after.len());
        format!("{}{}\n{}\n\n{}", &body[..after_start], "", transcription, &after[end..])
    } else {
        let sep = if body.ends_with('\n') { "" } else { "\n" };
        format!("{body}{sep}\n{header}\n{transcription}\n")
    };

    fs::write(path, format!("{prefix}{new_body}"))?;
    Ok(())
}

fn frontmatter_body_start(content: &str) -> usize {
    if !content.starts_with("---") {
        return 0;
    }
    content[3..]
        .find("---")
        .map(|idx| 3 + idx + 3 + content[3 + idx + 3..].chars().take_while(|c| *c == '\n').map(char::len_utf8).sum::<usize>())
        .unwrap_or(0)
}

fn encode_entry(path: &Path) -> DbResult<Vec<String>> {
    match path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("pdf") => encode_pdf(path),
        Some("jpg" | "jpeg") => encode_image_file(path, "image/jpeg").map(|one| vec![one]),
        Some("png") => encode_image_file(path, "image/png").map(|one| vec![one]),
        _ => Err(DbError::InvalidData(format!("unsupported transcription source: {}", path.display()))),
    }
}

fn encode_image_file(path: &Path, mime: &str) -> DbResult<String> {
    let bytes = fs::read(path)?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn encode_pdf(path: &Path) -> DbResult<Vec<String>> {
    let temp = tempfile::tempdir()?;
    let prefix = temp.path().join("page");
    let status = Command::new("pdftoppm")
        .arg("-png")
        .arg(path)
        .arg(&prefix)
        .status()?;
    if !status.success() {
        return Err(DbError::InvalidData(format!(
            "pdftoppm failed for {}; install poppler or convert PDF manually",
            path.display()
        )));
    }
    let mut pages = recursive_files(temp.path())?
        .into_iter()
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("png"))
        .collect::<Vec<_>>();
    pages.sort();
    pages.into_iter().map(|p| encode_image_file(&p, "image/png")).collect()
}

fn collect_tags(journal_dir: Option<&Path>, evergreen_dir: Option<&Path>) -> DbResult<String> {
    let tag_re = Regex::new(r"#([\w/-]+)").map_err(|err| DbError::InvalidData(err.to_string()))?;
    let mut tags = BTreeSet::new();
    for root in [journal_dir, evergreen_dir].into_iter().flatten() {
        if !root.exists() {
            continue;
        }
        for path in recursive_files(root)? {
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let content = fs::read_to_string(path)?;
            for cap in tag_re.captures_iter(&content) {
                if let Some(tag) = cap.get(1) {
                    tags.insert(tag.as_str().to_string());
                }
            }
        }
    }
    Ok(tags.into_iter().collect::<Vec<_>>().join(" "))
}

fn upsert_embedding(path: &Path, doc_path: &Path, embedding: Vec<f64>) -> DbResult<()> {
    let doc_path = path_to_string(doc_path);
    let mut rows = Vec::new();
    if path.exists() {
        let content = fs::read_to_string(path)?;
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let row = serde_json::from_str::<EmbeddingLine>(line)?;
            if row.path != doc_path {
                rows.push(row);
            }
        }
    }
    rows.push(EmbeddingLine { path: doc_path, embedding });
    let mut output = String::new();
    for row in rows {
        output.push_str(&json!(row).to_string());
        output.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    fs::write(&tmp, output)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
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
    fn strips_and_updates_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "hello").unwrap();
        update_frontmatter_field(&path, "embedding", "True").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(frontmatter_is_true(&parse_frontmatter(&content), "embedding"));
        assert_eq!(strip_frontmatter(&content), "hello");
    }

    #[test]
    fn upserts_embedding_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let embeddings = dir.path().join("embeddings.jsonl");
        let doc = dir.path().join("01-01-2024.md");
        upsert_embedding(&embeddings, &doc, vec![1.0]).unwrap();
        upsert_embedding(&embeddings, &doc, vec![2.0]).unwrap();
        let lines = fs::read_to_string(embeddings).unwrap();
        assert_eq!(lines.lines().count(), 1);
        assert!(lines.contains("2.0"));
    }
}
