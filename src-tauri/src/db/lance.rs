use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::Arc,
};

use arrow_array::{
    builder::{ListBuilder, StringBuilder},
    types::Float32Type,
    Array, ArrayRef, FixedSizeListArray, Float32Array, Float64Array, Int64Array, ListArray,
    RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use futures::TryStreamExt;
use lancedb::{
    database::CreateTableMode,
    index::Index,
    query::{ExecutableQuery, QueryBase, Select},
    Connection, DistanceType,
};
use uuid::Uuid;

use crate::{
    ingestion::{self, IngestionConfig},
    models::{
        Entry, LogEvent, Message, MessageMetadata, TagInstance, TagSummary, TaxonomyTag, Thread,
        UpdateTaxonomyTagRequest,
    },
};

use super::{ingest, ingest::JournalRow, DbError, DbResult};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DbConfig {
    pub lance_path: PathBuf,
    pub journal_dir: Option<PathBuf>,
    pub evergreen_dir: Option<PathBuf>,
    pub embeddings_path: Option<PathBuf>,
    pub ingest_on_startup: bool,
}

#[derive(Clone)]
pub struct Db {
    conn: Connection,
    #[allow(dead_code)]
    config: DbConfig,
}

impl Db {
    pub async fn connect(config: DbConfig) -> DbResult<Self> {
        std::fs::create_dir_all(&config.lance_path)?;
        let uri = config.lance_path.to_string_lossy();
        let conn = lancedb::connect(uri.as_ref()).execute().await?;
        Ok(Self { conn, config })
    }

    pub async fn startup_ingest(&self) -> DbResult<()> {
        self.ensure_threads_table().await?;
        self.ensure_messages_table().await?;
        self.ensure_log_events_table().await?;
        self.ensure_taxonomy_table().await?;
        if self.config.ingest_on_startup {
            ingestion::run_startup_pipeline(&IngestionConfig {
                journal_dir: self.config.journal_dir.clone(),
                evergreen_dir: self.config.evergreen_dir.clone(),
                embeddings_path: self.config.embeddings_path.clone(),
            })
            .await?;
        }
        self.incremental_journal_ingest().await?;
        self.sync_taxonomy().await?;
        Ok(())
    }

    pub async fn get_recent_entries(&self, n: usize) -> DbResult<Vec<Entry>> {
        if !self.table_exists("journal").await? {
            return Ok(vec![]);
        }
        let table = self.conn.open_table("journal").execute().await?;
        let stream = table
            .query()
            .only_if("entry_type != 'evergreen'")
            .execute()
            .await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let mut entries = entries_from_batches(&batches, false)?
            .into_iter()
            .map(|(entry, _)| entry)
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| b.date.cmp(&a.date));
        entries.truncate(n);
        Ok(entries)
    }

    pub async fn list_tags(&self) -> DbResult<Vec<TagSummary>> {
        let mut counts: HashMap<String, usize> = HashMap::new();

        if self.table_exists("journal").await? {
            let table = self.conn.open_table("journal").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for (entry, _) in entries_from_batches(&batches, false)? {
                for tag in entry.tags {
                    *counts.entry(tag).or_insert(0) += 1;
                }
            }
        }

        if self.table_exists("log_events").await? {
            let table = self.conn.open_table("log_events").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for event in log_events_from_batches(&batches)? {
                for tag in event.tags {
                    *counts.entry(tag).or_insert(0) += 1;
                }
            }
        }

        if self.table_exists("threads").await? {
            let table = self.conn.open_table("threads").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for thread in threads_from_batches(&batches)? {
                for tag in thread.tags.unwrap_or_default() {
                    *counts.entry(tag).or_insert(0) += 1;
                }
            }
        }

        let mut summaries = counts
            .into_iter()
            .map(|(tag, count)| TagSummary { tag, count })
            .collect::<Vec<_>>();
        summaries.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
        Ok(summaries)
    }

    pub async fn delete_tag(&self, tag: String) -> DbResult<()> {
        if tag.trim().is_empty() {
            return Err(DbError::InvalidData("tag cannot be empty".to_string()));
        }

        if self.table_exists("journal").await? {
            let table = self.conn.open_table("journal").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            let mut rows = journal_rows_from_batches(&batches)?
                .into_iter()
                .filter_map(|mut row| {
                    let original_len = row.tags.len();
                    row.tags.retain(|row_tag| row_tag != &tag);
                    (row.tags.len() != original_len).then_some(row)
                })
                .collect::<Vec<_>>();

            for row in &rows {
                table
                    .delete(&format!("entry_id = '{}'", escape_sql(&row.entry_id)))
                    .await?;
            }
            if !rows.is_empty() {
                table.add(journal_batch(&rows)?).execute().await?;
                rows.clear();
            }
        }

        if self.table_exists("log_events").await? {
            let table = self.conn.open_table("log_events").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            let events = log_events_from_batches(&batches)?
                .into_iter()
                .filter_map(|mut event| {
                    let original_len = event.tags.len();
                    event.tags.retain(|event_tag| event_tag != &tag);
                    (event.tags.len() != original_len).then_some(event)
                })
                .collect::<Vec<_>>();

            for event in &events {
                table
                    .delete(&format!(
                        "log_event_id = '{}'",
                        escape_sql(&event.log_event_id)
                    ))
                    .await?;
            }
            if !events.is_empty() {
                table.add(log_event_batch(&events)?).execute().await?;
            }
        }

        if self.table_exists("threads").await? {
            let table = self.conn.open_table("threads").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            let threads = threads_from_batches(&batches)?
                .into_iter()
                .filter_map(|mut thread| {
                    let mut tags = thread.tags.unwrap_or_default();
                    let original_len = tags.len();
                    tags.retain(|thread_tag| thread_tag != &tag);
                    if tags.len() == original_len {
                        return None;
                    }
                    thread.tags = Some(tags);
                    Some(thread)
                })
                .collect::<Vec<_>>();

            for thread in &threads {
                table
                    .delete(&format!("thread_id = '{}'", escape_sql(&thread.thread_id)))
                    .await?;
            }
            if !threads.is_empty() {
                table.add(thread_batch(&threads)?).execute().await?;
            }
        }

        Ok(())
    }

    pub async fn get_similar_entries(
        &self,
        embedding: Vec<f64>,
        n: usize,
    ) -> DbResult<Vec<(Entry, f64)>> {
        if !self.table_exists("journal").await? || embedding.is_empty() {
            return Ok(vec![]);
        }
        let query = embedding.into_iter().map(|v| v as f32).collect::<Vec<_>>();
        let table = self.conn.open_table("journal").execute().await?;
        let stream = table
            .query()
            .limit(n)
            .nearest_to(query.as_slice())?
            .distance_type(DistanceType::Cosine)
            .execute()
            .await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let mut entries = entries_from_batches(&batches, true)?;
        entries.sort_by(|a, b| {
            a.1.unwrap_or(f64::INFINITY)
                .total_cmp(&b.1.unwrap_or(f64::INFINITY))
        });
        Ok(entries
            .into_iter()
            .map(|(entry, distance)| (entry, distance.unwrap_or(0.0)))
            .collect())
    }

    pub async fn get_entries_by_date_range(
        &self,
        start_date: String,
        end_date: String,
        n: Option<usize>,
    ) -> DbResult<Vec<Entry>> {
        if !self.table_exists("journal").await? {
            return Ok(vec![]);
        }
        let table = self.conn.open_table("journal").execute().await?;
        let stream = table
            .query()
            .only_if(format!(
                "date >= '{}' AND date <= '{}'",
                escape_sql(&start_date),
                escape_sql(&end_date)
            ))
            .execute()
            .await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let mut entries = entries_from_batches(&batches, false)?
            .into_iter()
            .map(|(entry, _)| entry)
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| b.date.cmp(&a.date));
        if let Some(n) = n {
            entries.truncate(n);
        }
        Ok(entries)
    }

    pub async fn create_thread(
        &self,
        title: Option<String>,
        initial_message: Option<String>,
    ) -> DbResult<Thread> {
        let now = Utc::now().to_rfc3339();
        let thread = Thread {
            thread_id: Uuid::new_v4().to_string(),
            title: title.unwrap_or_else(|| format!("Chat {}", Utc::now().format("%Y-%m-%d %H:%M"))),
            tags: Some(vec![]),
            created_at: now.clone(),
            updated_at: now,
        };

        let table = self.conn.open_table("threads").execute().await?;
        table
            .add(thread_batch(&[thread.clone()])?)
            .execute()
            .await?;

        if let Some(content) = initial_message {
            self.save_message(thread.thread_id.clone(), "user".to_string(), content, None)
                .await?;
        }

        Ok(thread)
    }

    pub async fn get_threads(&self) -> DbResult<Vec<Thread>> {
        let table = self.conn.open_table("threads").execute().await?;
        let stream = table
            .query()
            .select(Select::columns(&[
                "thread_id",
                "title",
                "created_at",
                "updated_at",
            ]))
            .execute()
            .await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let mut threads = threads_from_batches(&batches)?;
        threads.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(threads)
    }

    pub async fn get_thread(&self, thread_id: String) -> DbResult<Option<Thread>> {
        let table = self.conn.open_table("threads").execute().await?;
        let stream = table
            .query()
            .select(Select::columns(&[
                "thread_id",
                "title",
                "created_at",
                "updated_at",
            ]))
            .only_if(format!("thread_id = '{}'", escape_sql(&thread_id)))
            .execute()
            .await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        Ok(threads_from_batches(&batches)?.into_iter().next())
    }

    pub async fn update_thread_title(&self, thread_id: String, title: String) -> DbResult<()> {
        let Some(mut thread) = self.get_thread(thread_id.clone()).await? else {
            return Err(DbError::InvalidData(format!(
                "thread not found: {thread_id}"
            )));
        };

        thread.title = title;
        thread.updated_at = Utc::now().to_rfc3339();

        let table = self.conn.open_table("threads").execute().await?;
        table
            .delete(&format!("thread_id = '{}'", escape_sql(&thread_id)))
            .await?;
        table.add(thread_batch(&[thread])?).execute().await?;
        Ok(())
    }

    pub async fn delete_thread(&self, thread_id: String) -> DbResult<()> {
        let pred = format!("thread_id = '{}'", escape_sql(&thread_id));
        let threads = self.conn.open_table("threads").execute().await?;
        let messages = self.conn.open_table("messages").execute().await?;
        threads.delete(&pred).await?;
        messages.delete(&pred).await?;
        Ok(())
    }

    pub async fn save_message(
        &self,
        thread_id: String,
        role: String,
        content: String,
        metadata: Option<MessageMetadata>,
    ) -> DbResult<Message> {
        let message = Message {
            message_id: Uuid::new_v4().to_string(),
            thread_id: thread_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            role,
            content,
            metadata: metadata.map(compact_message_metadata),
        };

        let table = self.conn.open_table("messages").execute().await?;
        table
            .add(message_batch(&[message.clone()])?)
            .execute()
            .await?;

        if let Some(mut thread) = self.get_thread(thread_id.clone()).await? {
            thread.updated_at = Utc::now().to_rfc3339();
            let threads = self.conn.open_table("threads").execute().await?;
            threads
                .delete(&format!("thread_id = '{}'", escape_sql(&thread_id)))
                .await?;
            threads.add(thread_batch(&[thread])?).execute().await?;
        }

        Ok(message)
    }

    pub async fn get_thread_messages(&self, thread_id: String) -> DbResult<Vec<Message>> {
        let table = self.conn.open_table("messages").execute().await?;
        let stream = table
            .query()
            .only_if(format!("thread_id = '{}'", escape_sql(&thread_id)))
            .execute()
            .await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let mut messages = messages_from_batches(&batches)?;
        messages.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        Ok(messages)
    }

    pub async fn create_log_event(&self, mut event: LogEvent) -> DbResult<LogEvent> {
        if event.text.trim().is_empty() {
            return Err(DbError::InvalidData(
                "log event text cannot be empty".to_string(),
            ));
        }
        if event.datetime.trim().is_empty() {
            event.datetime = Utc::now().to_rfc3339();
        }
        event.log_event_id = Uuid::new_v4().to_string();

        let table = self.conn.open_table("log_events").execute().await?;
        table
            .add(log_event_batch(&[event.clone()])?)
            .execute()
            .await?;
        self.sync_taxonomy().await?;
        Ok(event)
    }

    pub async fn delete_log_event(&self, log_event_id: String) -> DbResult<()> {
        if log_event_id.trim().is_empty() {
            return Err(DbError::InvalidData(
                "log event id cannot be empty".to_string(),
            ));
        }

        let table = self.conn.open_table("log_events").execute().await?;
        table
            .delete(&format!("log_event_id = '{}'", escape_sql(&log_event_id)))
            .await?;
        self.sync_taxonomy().await?;
        Ok(())
    }

    pub async fn list_log_events(
        &self,
        order: Option<String>,
        limit: Option<usize>,
        tags: Option<Vec<String>>,
    ) -> DbResult<Vec<LogEvent>> {
        let table = self.conn.open_table("log_events").execute().await?;
        let stream = table.query().execute().await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let required_tags = tags.unwrap_or_default();
        let mut events = log_events_from_batches(&batches)?
            .into_iter()
            .filter(|event| {
                required_tags
                    .iter()
                    .all(|tag| event.tags.iter().any(|event_tag| event_tag == tag))
            })
            .collect::<Vec<_>>();

        match order.as_deref().unwrap_or("descending") {
            "ascending" | "asc" => events.sort_by(|a, b| a.datetime.cmp(&b.datetime)),
            "descending" | "desc" => events.sort_by(|a, b| b.datetime.cmp(&a.datetime)),
            other => {
                return Err(DbError::InvalidData(format!(
                    "invalid log event order: {other}"
                )))
            }
        }

        if let Some(limit) = limit {
            events.truncate(limit);
        }
        Ok(events)
    }

    pub async fn list_taxonomy_tags(&self) -> DbResult<Vec<TaxonomyTag>> {
        self.sync_taxonomy().await?;
        let rows = self
            .existing_taxonomy_tags()
            .await?
            .into_values()
            .collect::<Vec<_>>();
        let counts = self.taxonomy_counts().await?;
        Ok(enrich_taxonomy_tags(rows, &counts))
    }

    pub async fn get_taxonomy_tag(&self, tag: String) -> DbResult<Option<TaxonomyTag>> {
        let tag = normalize_tag_required(&tag)?;
        Ok(self
            .list_taxonomy_tags()
            .await?
            .into_iter()
            .find(|row| row.tag == tag))
    }

    pub async fn update_taxonomy_tag(
        &self,
        req: UpdateTaxonomyTagRequest,
    ) -> DbResult<TaxonomyTag> {
        let tag = normalize_tag_required(&req.tag)?;
        self.ensure_taxonomy_table().await?;

        let table = self.conn.open_table("taxonomy").execute().await?;
        table
            .delete(&format!("tag = '{}'", escape_sql(&tag)))
            .await?;
        table
            .add(taxonomy_batch(&[TaxonomyTag {
                tag: tag.clone(),
                description: req.description,
                color: req.color,
                broader: vec![],
                narrower: vec![],
                count: 0,
            }])?)
            .execute()
            .await?;

        self.get_taxonomy_tag(tag).await?.ok_or_else(|| {
            DbError::InvalidData("updated taxonomy tag could not be read".to_string())
        })
    }

    pub async fn rename_taxonomy_tag(
        &self,
        old_tag: String,
        new_tag: String,
    ) -> DbResult<TaxonomyTag> {
        let old_tag = validate_taxonomy_tag(&old_tag)?;
        let new_tag = validate_taxonomy_tag(&new_tag)?;

        if old_tag == new_tag {
            return Err(DbError::InvalidData(
                "old and new taxonomy tags must differ".to_string(),
            ));
        }

        self.rename_source_tags(&old_tag, &new_tag).await?;
        self.rename_taxonomy_rows(&old_tag, &new_tag).await?;
        self.sync_taxonomy().await?;
        self.get_taxonomy_tag(new_tag).await?.ok_or_else(|| {
            DbError::InvalidData("renamed taxonomy tag could not be read".to_string())
        })
    }

    async fn rename_source_tags(&self, old_tag: &str, new_tag: &str) -> DbResult<()> {
        self.rename_journal_tags(old_tag, new_tag).await?;
        self.rename_log_event_tags(old_tag, new_tag).await?;
        self.rename_thread_tags(old_tag, new_tag).await?;
        Ok(())
    }

    async fn rename_journal_tags(&self, old_tag: &str, new_tag: &str) -> DbResult<()> {
        if !self.table_exists("journal").await? {
            return Ok(());
        }

        let table = self.conn.open_table("journal").execute().await?;
        let stream = table.query().execute().await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let rows = journal_rows_from_batches(&batches)?
            .into_iter()
            .filter_map(|mut row| {
                let (tags, changed) = rename_tags_in_array(row.tags, old_tag, new_tag);
                if !changed {
                    return None;
                }
                row.tags = tags;
                Some(row)
            })
            .collect::<Vec<_>>();

        for row in &rows {
            table
                .delete(&format!("entry_id = '{}'", escape_sql(&row.entry_id)))
                .await?;
        }
        if !rows.is_empty() {
            table.add(journal_batch(&rows)?).execute().await?;
        }
        Ok(())
    }

    async fn rename_log_event_tags(&self, old_tag: &str, new_tag: &str) -> DbResult<()> {
        if !self.table_exists("log_events").await? {
            return Ok(());
        }

        let table = self.conn.open_table("log_events").execute().await?;
        let stream = table.query().execute().await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let events = log_events_from_batches(&batches)?
            .into_iter()
            .filter_map(|mut event| {
                let (tags, changed) = rename_tags_in_array(event.tags, old_tag, new_tag);
                if !changed {
                    return None;
                }
                event.tags = tags;
                Some(event)
            })
            .collect::<Vec<_>>();

        for event in &events {
            table
                .delete(&format!(
                    "log_event_id = '{}'",
                    escape_sql(&event.log_event_id)
                ))
                .await?;
        }
        if !events.is_empty() {
            table.add(log_event_batch(&events)?).execute().await?;
        }
        Ok(())
    }

    async fn rename_thread_tags(&self, old_tag: &str, new_tag: &str) -> DbResult<()> {
        if !self.table_exists("threads").await? {
            return Ok(());
        }

        let table = self.conn.open_table("threads").execute().await?;
        let stream = table.query().execute().await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        let threads = threads_from_batches(&batches)?
            .into_iter()
            .filter_map(|mut thread| {
                let (tags, changed) =
                    rename_tags_in_array(thread.tags.unwrap_or_default(), old_tag, new_tag);
                if !changed {
                    return None;
                }
                thread.tags = Some(tags);
                Some(thread)
            })
            .collect::<Vec<_>>();

        for thread in &threads {
            table
                .delete(&format!("thread_id = '{}'", escape_sql(&thread.thread_id)))
                .await?;
        }
        if !threads.is_empty() {
            table.add(thread_batch(&threads)?).execute().await?;
        }
        Ok(())
    }

    async fn rename_taxonomy_rows(&self, old_tag: &str, new_tag: &str) -> DbResult<()> {
        self.ensure_taxonomy_table().await?;
        let existing = self.existing_taxonomy_tags().await?;
        if existing.is_empty() {
            return Ok(());
        }

        let mut merged = HashMap::<String, TaxonomyTag>::new();
        let mut touched = BTreeSet::<String>::new();

        for row in existing.values() {
            if !is_tag_or_descendant(&row.tag, old_tag) {
                merged.insert(row.tag.clone(), row.clone());
            }
        }

        for row in existing.values() {
            let Some(target) = rename_tag_path(&row.tag, old_tag, new_tag) else {
                continue;
            };
            touched.insert(row.tag.clone());
            touched.insert(target.clone());

            let mut renamed = row.clone();
            renamed.tag = target.clone();
            renamed.broader = vec![];
            renamed.narrower = vec![];
            renamed.count = 0;

            if let Some(destination) = merged.remove(&target) {
                merged.insert(
                    target.clone(),
                    merge_taxonomy_metadata(destination, renamed),
                );
            } else {
                merged.insert(target.clone(), renamed);
            }
        }

        let rows_to_add = touched
            .iter()
            .filter_map(|tag| merged.get(tag).cloned())
            .collect::<Vec<_>>();

        let table = self.conn.open_table("taxonomy").execute().await?;
        for tag in touched {
            table
                .delete(&format!("tag = '{}'", escape_sql(&tag)))
                .await?;
        }
        if !rows_to_add.is_empty() {
            table.add(taxonomy_batch(&rows_to_add)?).execute().await?;
        }
        Ok(())
    }

    pub async fn resolve_tag_colors(
        &self,
        tags: Vec<String>,
    ) -> DbResult<HashMap<String, Option<String>>> {
        self.ensure_taxonomy_table().await?;
        let taxonomy = self.existing_taxonomy_tags().await?;
        let mut resolved = HashMap::new();

        for tag in tags {
            let Some(normalized) = normalize_tag(&tag) else {
                continue;
            };
            if resolved.contains_key(&normalized) {
                continue;
            }

            let mut current = Some(normalized.clone());
            let mut color = None;
            while let Some(candidate) = current {
                if let Some(taxonomy_tag) = taxonomy.get(&candidate) {
                    if taxonomy_tag.color.is_some() {
                        color = taxonomy_tag.color.clone();
                        break;
                    }
                }
                current = parent_tag(&candidate);
            }

            resolved.insert(normalized, color);
        }

        Ok(resolved)
    }

    pub async fn list_tag_instances(
        &self,
        tag: String,
        limit: Option<usize>,
    ) -> DbResult<Vec<TagInstance>> {
        let tag = normalize_tag_required(&tag)?;
        let mut instances = Vec::new();

        if self.table_exists("log_events").await? {
            let table = self.conn.open_table("log_events").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for event in log_events_from_batches(&batches)? {
                if tags_contain(&event.tags, &tag) {
                    instances.push(TagInstance {
                        tag: tag.clone(),
                        source_type: "log_event".to_string(),
                        source_id: event.log_event_id,
                        title: None,
                        text: Some(event.text),
                        datetime: Some(event.datetime),
                    });
                }
            }
        }

        if self.table_exists("journal").await? {
            let table = self.conn.open_table("journal").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for (entry, _) in entries_from_batches(&batches, false)? {
                if tags_contain(&entry.tags, &tag) {
                    instances.push(TagInstance {
                        tag: tag.clone(),
                        source_type: "journal".to_string(),
                        source_id: entry.entry_id,
                        title: Some(entry.title),
                        text: Some(entry.text),
                        datetime: Some(entry.date),
                    });
                }
            }
        }

        if self.table_exists("threads").await? {
            let table = self.conn.open_table("threads").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for thread in threads_from_batches(&batches)? {
                if tags_contain(&thread.tags.clone().unwrap_or_default(), &tag) {
                    instances.push(TagInstance {
                        tag: tag.clone(),
                        source_type: "thread".to_string(),
                        source_id: thread.thread_id,
                        title: Some(thread.title),
                        text: None,
                        datetime: Some(thread.updated_at),
                    });
                }
            }
        }

        instances.sort_by(|a, b| b.datetime.cmp(&a.datetime));
        if let Some(limit) = limit {
            instances.truncate(limit);
        }
        Ok(instances)
    }

    pub async fn sync_taxonomy(&self) -> DbResult<()> {
        self.ensure_taxonomy_table().await?;

        let used_tags = self.collect_used_taxonomy_tags().await?;
        let existing = self.existing_taxonomy_tags().await?;
        let missing = used_tags
            .into_iter()
            .filter(|tag| !existing.contains_key(tag))
            .map(|tag| TaxonomyTag {
                tag,
                description: String::new(),
                color: None,
                broader: vec![],
                narrower: vec![],
                count: 0,
            })
            .collect::<Vec<_>>();

        if !missing.is_empty() {
            let table = self.conn.open_table("taxonomy").execute().await?;
            table.add(taxonomy_batch(&missing)?).execute().await?;
        }

        Ok(())
    }

    async fn collect_used_taxonomy_tags(&self) -> DbResult<BTreeSet<String>> {
        let mut tags = BTreeSet::new();

        if self.table_exists("journal").await? {
            let table = self.conn.open_table("journal").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for (entry, _) in entries_from_batches(&batches, false)? {
                for tag in entry.tags {
                    tags.extend(tag_with_ancestors(&tag));
                }
            }
        }

        if self.table_exists("log_events").await? {
            let table = self.conn.open_table("log_events").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for event in log_events_from_batches(&batches)? {
                for tag in event.tags {
                    tags.extend(tag_with_ancestors(&tag));
                }
            }
        }

        if self.table_exists("threads").await? {
            let table = self.conn.open_table("threads").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for thread in threads_from_batches(&batches)? {
                for tag in thread.tags.unwrap_or_default() {
                    tags.extend(tag_with_ancestors(&tag));
                }
            }
        }

        Ok(tags)
    }

    async fn taxonomy_counts(&self) -> DbResult<HashMap<String, usize>> {
        let mut counts = HashMap::new();

        if self.table_exists("journal").await? {
            let table = self.conn.open_table("journal").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for (entry, _) in entries_from_batches(&batches, false)? {
                for tag in entry.tags {
                    if let Some(tag) = normalize_tag(&tag) {
                        *counts.entry(tag).or_insert(0) += 1;
                    }
                }
            }
        }

        if self.table_exists("log_events").await? {
            let table = self.conn.open_table("log_events").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for event in log_events_from_batches(&batches)? {
                for tag in event.tags {
                    if let Some(tag) = normalize_tag(&tag) {
                        *counts.entry(tag).or_insert(0) += 1;
                    }
                }
            }
        }

        if self.table_exists("threads").await? {
            let table = self.conn.open_table("threads").execute().await?;
            let stream = table.query().execute().await?;
            let batches = stream.try_collect::<Vec<_>>().await?;
            for thread in threads_from_batches(&batches)? {
                for tag in thread.tags.unwrap_or_default() {
                    if let Some(tag) = normalize_tag(&tag) {
                        *counts.entry(tag).or_insert(0) += 1;
                    }
                }
            }
        }

        Ok(counts)
    }

    async fn existing_taxonomy_tags(&self) -> DbResult<HashMap<String, TaxonomyTag>> {
        if !self.table_exists("taxonomy").await? {
            return Ok(HashMap::new());
        }

        let table = self.conn.open_table("taxonomy").execute().await?;
        let stream = table.query().execute().await?;
        let batches = stream.try_collect::<Vec<_>>().await?;
        Ok(taxonomy_tags_from_batches(&batches)?
            .into_iter()
            .map(|tag| (tag.tag.clone(), tag))
            .collect())
    }

    async fn ensure_threads_table(&self) -> DbResult<()> {
        if self.table_exists("threads").await? {
            return Ok(());
        }
        self.conn
            .create_empty_table("threads", thread_schema())
            .mode(CreateTableMode::Create)
            .execute()
            .await?;
        Ok(())
    }

    async fn ensure_messages_table(&self) -> DbResult<()> {
        if self.table_exists("messages").await? {
            return Ok(());
        }
        self.conn
            .create_empty_table("messages", message_schema())
            .mode(CreateTableMode::Create)
            .execute()
            .await?;
        Ok(())
    }

    async fn ensure_log_events_table(&self) -> DbResult<()> {
        if self.table_exists("log_events").await? {
            return Ok(());
        }
        self.conn
            .create_empty_table("log_events", log_event_schema())
            .mode(CreateTableMode::Create)
            .execute()
            .await?;
        Ok(())
    }

    async fn ensure_taxonomy_table(&self) -> DbResult<()> {
        if self.table_exists("taxonomy").await? {
            return Ok(());
        }
        self.conn
            .create_empty_table("taxonomy", taxonomy_schema())
            .mode(CreateTableMode::Create)
            .execute()
            .await?;
        Ok(())
    }

    async fn incremental_journal_ingest(&self) -> DbResult<()> {
        let Some(embeddings_path) = self.config.embeddings_path.as_deref() else {
            eprintln!("[lancedb] ZENFROG_EMBEDDINGS_PATH unset; skipping journal ingest");
            return Ok(());
        };
        if !embeddings_path.exists() {
            eprintln!(
                "[lancedb] embeddings file does not exist, skipping journal ingest: {}",
                embeddings_path.display()
            );
            return Ok(());
        }
        if self.config.journal_dir.is_none() && self.config.evergreen_dir.is_none() {
            eprintln!("[lancedb] journal/evergreen dirs unset; skipping journal ingest");
            return Ok(());
        }

        let rows = ingest::load_journal_rows(
            self.config.journal_dir.as_deref(),
            self.config.evergreen_dir.as_deref(),
            embeddings_path,
        )?;
        if rows.is_empty() {
            eprintln!(
                "[lancedb] no journal rows discovered; leaving existing journal table unchanged"
            );
            return Ok(());
        }

        if !self.table_exists("journal").await? {
            let table = self
                .conn
                .create_table("journal", journal_batch(&rows)?)
                .mode(CreateTableMode::Create)
                .execute()
                .await?;
            ensure_journal_index(&table).await?;
            eprintln!("[lancedb] created journal table with {} rows", rows.len());
            return Ok(());
        }

        let table = self.conn.open_table("journal").execute().await?;
        let existing = existing_journal_hashes(&table).await?;
        let desired = rows
            .iter()
            .map(|row| row.entry_id.clone())
            .collect::<std::collections::HashSet<_>>();

        let mut rows_to_add = Vec::new();
        for row in rows {
            match existing.get(&row.entry_id) {
                Some(hash) if hash == &row.content_hash => {}
                Some(_) => {
                    table
                        .delete(&format!("entry_id = '{}'", escape_sql(&row.entry_id)))
                        .await?;
                    rows_to_add.push(row);
                }
                None => rows_to_add.push(row),
            }
        }

        let mut deleted = 0usize;
        for entry_id in existing.keys() {
            if !desired.contains(entry_id) {
                table
                    .delete(&format!("entry_id = '{}'", escape_sql(entry_id)))
                    .await?;
                deleted += 1;
            }
        }

        let added = rows_to_add.len();
        if !rows_to_add.is_empty() {
            table.add(journal_batch(&rows_to_add)?).execute().await?;
        }
        if added > 0 || deleted > 0 {
            ensure_journal_index(&table).await?;
        }
        eprintln!("[lancedb] incremental journal ingest complete: {added} added/updated, {deleted} deleted");
        Ok(())
    }

    async fn table_exists(&self, name: &str) -> DbResult<bool> {
        let names = self.conn.table_names().execute().await?;
        Ok(names.iter().any(|table| table == name))
    }
}

async fn ensure_journal_index(table: &lancedb::Table) -> DbResult<()> {
    let has_embedding_index = table
        .list_indices()
        .await?
        .iter()
        .any(|idx| idx.columns.iter().any(|col| col == "embedding"));
    if !has_embedding_index {
        if let Err(err) = table
            .create_index(&["embedding"], Index::Auto)
            .execute()
            .await
        {
            // Tiny local/test datasets can be below Lance's training threshold for PQ.
            // LanceDB will still do exhaustive vector search, so don't brick startup.
            let message = err.to_string();
            if message.contains("Not enough rows to train") {
                eprintln!("[lancedb] skipping vector index for small journal table: {message}");
            } else {
                return Err(err.into());
            }
        }
    }
    Ok(())
}

async fn existing_journal_hashes(table: &lancedb::Table) -> DbResult<HashMap<String, String>> {
    let stream = table
        .query()
        .select(Select::columns(&["entry_id", "content_hash"]))
        .execute()
        .await?;
    let batches = stream.try_collect::<Vec<_>>().await?;
    let mut out = HashMap::new();
    for batch in batches {
        let entry_ids = string_column(&batch, "entry_id")?;
        let content_hashes = string_column(&batch, "content_hash")?;
        for i in 0..batch.num_rows() {
            out.insert(string_value(entry_ids, i), string_value(content_hashes, i));
        }
    }
    Ok(out)
}

fn journal_schema(dim: usize) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("entry_id", DataType::Utf8, false),
        Field::new("date", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "tags",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            true,
        ),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dim as i32,
            ),
            true,
        ),
        Field::new("entry_type", DataType::Utf8, false),
        Field::new("source_path", DataType::Utf8, false),
        Field::new("embedding_key", DataType::Utf8, false),
        Field::new("content_hash", DataType::Utf8, false),
        Field::new("source_mtime_ms", DataType::Int64, false),
        Field::new("indexed_at", DataType::Utf8, false),
    ]))
}

fn journal_batch(rows: &[JournalRow]) -> DbResult<Box<dyn arrow_array::RecordBatchReader + Send>> {
    let dim = rows
        .first()
        .map(|row| row.embedding.len())
        .ok_or_else(|| DbError::InvalidData("cannot create empty journal batch".to_string()))?;
    if rows.iter().any(|row| row.embedding.len() != dim) {
        return Err(DbError::InvalidData(
            "journal rows have mixed embedding dimensions".to_string(),
        ));
    }

    let schema = journal_schema(dim);
    let tags = rows.iter().map(|row| row.tags.clone()).collect::<Vec<_>>();
    let embeddings = rows
        .iter()
        .map(|row| Some(row.embedding.iter().map(|v| Some(*v)).collect::<Vec<_>>()))
        .collect::<Vec<_>>();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.entry_id.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.date.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            string_list_array(&tags),
            Arc::new(
                FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                    embeddings, dim as i32,
                ),
            ) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.entry_type.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.source_path.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.embedding_key.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.content_hash.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from_iter_values(
                rows.iter().map(|r| r.source_mtime_ms),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.indexed_at.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )?;

    Ok(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
}

fn taxonomy_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("tag", DataType::Utf8, false),
        Field::new("description", DataType::Utf8, false),
        Field::new("color", DataType::Utf8, true),
    ]))
}

fn thread_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("thread_id", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new(
            "tags",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            true,
        ),
        Field::new("created_at", DataType::Utf8, false),
        Field::new("updated_at", DataType::Utf8, false),
    ]))
}

fn message_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("message_id", DataType::Utf8, false),
        Field::new("thread_id", DataType::Utf8, false),
        Field::new("timestamp", DataType::Utf8, false),
        Field::new("role", DataType::Utf8, false),
        Field::new("content", DataType::Utf8, false),
        Field::new("metadata_json", DataType::Utf8, true),
    ]))
}

fn log_event_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("log_event_id", DataType::Utf8, false),
        Field::new("datetime", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "tags",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            true,
        ),
    ]))
}

fn taxonomy_batch(
    tags: &[TaxonomyTag],
) -> DbResult<Box<dyn arrow_array::RecordBatchReader + Send>> {
    let schema = taxonomy_schema();
    let colors = tags
        .iter()
        .map(|tag| tag.color.as_deref())
        .collect::<Vec<_>>();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(
                tags.iter().map(|t| t.tag.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                tags.iter()
                    .map(|t| t.description.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(colors)) as ArrayRef,
        ],
    )?;

    Ok(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
}

fn thread_batch(threads: &[Thread]) -> DbResult<Box<dyn arrow_array::RecordBatchReader + Send>> {
    let schema = thread_schema();
    let tags = threads
        .iter()
        .map(|thread| thread.tags.clone().unwrap_or_default())
        .collect::<Vec<_>>();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(
                threads
                    .iter()
                    .map(|t| t.thread_id.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                threads.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            string_list_array(&tags),
            Arc::new(StringArray::from(
                threads
                    .iter()
                    .map(|t| t.created_at.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                threads
                    .iter()
                    .map(|t| t.updated_at.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )?;

    Ok(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
}

fn compact_message_metadata(mut metadata: MessageMetadata) -> MessageMetadata {
    for entry in &mut metadata.context_entries {
        if entry.entry_id.is_some() {
            entry.text = None;
        }
    }
    metadata
}

fn message_batch(messages: &[Message]) -> DbResult<Box<dyn arrow_array::RecordBatchReader + Send>> {
    let schema = message_schema();
    let metadata_json = messages
        .iter()
        .map(|message| {
            message
                .metadata
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let metadata_refs = metadata_json
        .iter()
        .map(|value| value.as_deref())
        .collect::<Vec<_>>();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(
                messages
                    .iter()
                    .map(|m| m.message_id.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                messages
                    .iter()
                    .map(|m| m.thread_id.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                messages
                    .iter()
                    .map(|m| m.timestamp.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                messages
                    .iter()
                    .map(|m| m.content.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(metadata_refs)) as ArrayRef,
        ],
    )?;

    Ok(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
}

fn log_event_batch(
    events: &[LogEvent],
) -> DbResult<Box<dyn arrow_array::RecordBatchReader + Send>> {
    let schema = log_event_schema();
    let tags = events
        .iter()
        .map(|event| event.tags.clone())
        .collect::<Vec<_>>();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(
                events
                    .iter()
                    .map(|event| event.log_event_id.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                events
                    .iter()
                    .map(|event| event.datetime.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                events
                    .iter()
                    .map(|event| event.text.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            string_list_array(&tags),
        ],
    )?;

    Ok(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
}

fn string_list_array(rows: &[Vec<String>]) -> ArrayRef {
    let values = StringBuilder::new();
    let mut builder = ListBuilder::new(values);
    for row in rows {
        for item in row {
            builder.values().append_value(item);
        }
        builder.append(true);
    }
    Arc::new(builder.finish()) as ArrayRef
}

fn taxonomy_tags_from_batches(batches: &[RecordBatch]) -> DbResult<Vec<TaxonomyTag>> {
    let mut tags = Vec::new();
    for batch in batches {
        let tag_values = string_column(batch, "tag")?;
        let descriptions = string_column(batch, "description")?;
        let colors = string_column(batch, "color")?;

        for i in 0..batch.num_rows() {
            tags.push(TaxonomyTag {
                tag: string_value(tag_values, i),
                description: string_value(descriptions, i),
                color: if colors.is_null(i) {
                    None
                } else {
                    Some(colors.value(i).to_string())
                },
                broader: vec![],
                narrower: vec![],
                count: 0,
            });
        }
    }
    Ok(tags)
}

fn threads_from_batches(batches: &[RecordBatch]) -> DbResult<Vec<Thread>> {
    let mut threads = Vec::new();
    for batch in batches {
        let thread_ids = string_column(batch, "thread_id")?;
        let titles = string_column(batch, "title")?;
        let created = string_column(batch, "created_at")?;
        let updated = string_column(batch, "updated_at")?;
        let tags = batch
            .schema()
            .index_of("tags")
            .ok()
            .and_then(|_| list_string_column(batch, "tags").ok());

        for i in 0..batch.num_rows() {
            threads.push(Thread {
                thread_id: string_value(thread_ids, i),
                title: string_value(titles, i),
                tags: Some(
                    tags.map(|tags| list_string_value(tags, i))
                        .unwrap_or_default(),
                ),
                created_at: string_value(created, i),
                updated_at: string_value(updated, i),
            });
        }
    }
    Ok(threads)
}

fn messages_from_batches(batches: &[RecordBatch]) -> DbResult<Vec<Message>> {
    let mut messages = Vec::new();
    for batch in batches {
        let message_ids = string_column(batch, "message_id")?;
        let thread_ids = string_column(batch, "thread_id")?;
        let timestamps = string_column(batch, "timestamp")?;
        let roles = string_column(batch, "role")?;
        let contents = string_column(batch, "content")?;
        let metadata_json = string_column(batch, "metadata_json")?;

        for i in 0..batch.num_rows() {
            let metadata = if metadata_json.is_null(i) {
                None
            } else {
                Some(serde_json::from_str::<MessageMetadata>(
                    metadata_json.value(i),
                )?)
            };
            messages.push(Message {
                message_id: string_value(message_ids, i),
                thread_id: string_value(thread_ids, i),
                timestamp: string_value(timestamps, i),
                role: string_value(roles, i),
                content: string_value(contents, i),
                metadata,
            });
        }
    }
    Ok(messages)
}

fn log_events_from_batches(batches: &[RecordBatch]) -> DbResult<Vec<LogEvent>> {
    let mut events = Vec::new();
    for batch in batches {
        let log_event_ids = string_column(batch, "log_event_id")?;
        let datetimes = string_column(batch, "datetime")?;
        let texts = string_column(batch, "text")?;
        let tags = list_string_column(batch, "tags")?;

        for i in 0..batch.num_rows() {
            events.push(LogEvent {
                log_event_id: string_value(log_event_ids, i),
                datetime: string_value(datetimes, i),
                text: string_value(texts, i),
                tags: list_string_value(tags, i),
            });
        }
    }
    Ok(events)
}

fn entries_from_batches(
    batches: &[RecordBatch],
    include_distance: bool,
) -> DbResult<Vec<(Entry, Option<f64>)>> {
    let mut entries = Vec::new();
    for batch in batches {
        let entry_ids = string_column(batch, "entry_id")?;
        let dates = string_column(batch, "date")?;
        let titles = string_column(batch, "title")?;
        let texts = string_column(batch, "text")?;
        let tags = list_string_column(batch, "tags")?;
        let embeddings = fixed_f32_column(batch, "embedding")?;
        let entry_types = string_column(batch, "entry_type")?;
        let distances = if include_distance {
            float_column(batch, "_distance").ok()
        } else {
            None
        };

        for i in 0..batch.num_rows() {
            entries.push((
                Entry {
                    entry_id: string_value(entry_ids, i),
                    date: string_value(dates, i),
                    title: string_value(titles, i),
                    text: string_value(texts, i),
                    tags: list_string_value(tags, i),
                    embedding: Some(
                        fixed_f32_value(embeddings, i)
                            .into_iter()
                            .map(|v| v as f64)
                            .collect(),
                    ),
                    entry_type: string_value(entry_types, i),
                },
                distances.as_ref().map(|array| float_value(*array, i)),
            ));
        }
    }
    Ok(entries)
}

fn journal_rows_from_batches(batches: &[RecordBatch]) -> DbResult<Vec<JournalRow>> {
    let mut rows = Vec::new();
    for batch in batches {
        let entry_ids = string_column(batch, "entry_id")?;
        let dates = string_column(batch, "date")?;
        let titles = string_column(batch, "title")?;
        let texts = string_column(batch, "text")?;
        let tags = list_string_column(batch, "tags")?;
        let embeddings = fixed_f32_column(batch, "embedding")?;
        let entry_types = string_column(batch, "entry_type")?;
        let source_paths = string_column(batch, "source_path")?;
        let embedding_keys = string_column(batch, "embedding_key")?;
        let content_hashes = string_column(batch, "content_hash")?;
        let source_mtime_ms = int_column(batch, "source_mtime_ms")?;
        let indexed_ats = string_column(batch, "indexed_at")?;

        for i in 0..batch.num_rows() {
            rows.push(JournalRow {
                entry_id: string_value(entry_ids, i),
                date: string_value(dates, i),
                title: string_value(titles, i),
                text: string_value(texts, i),
                tags: list_string_value(tags, i),
                embedding: fixed_f32_value(embeddings, i),
                entry_type: string_value(entry_types, i),
                source_path: string_value(source_paths, i),
                embedding_key: string_value(embedding_keys, i),
                content_hash: string_value(content_hashes, i),
                source_mtime_ms: int_value(source_mtime_ms, i),
                indexed_at: string_value(indexed_ats, i),
            });
        }
    }
    Ok(rows)
}

fn string_column<'a>(batch: &'a RecordBatch, column: &str) -> DbResult<&'a StringArray> {
    let idx = batch
        .schema()
        .index_of(column)
        .map_err(|_| DbError::MissingColumn(column.to_string()))?;
    batch
        .column(idx)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| DbError::InvalidData(format!("column {column} is not utf8")))
}

fn string_value(array: &StringArray, row: usize) -> String {
    if array.is_null(row) {
        String::new()
    } else {
        array.value(row).to_string()
    }
}

fn list_string_column<'a>(batch: &'a RecordBatch, column: &str) -> DbResult<&'a ListArray> {
    let idx = batch
        .schema()
        .index_of(column)
        .map_err(|_| DbError::MissingColumn(column.to_string()))?;
    batch
        .column(idx)
        .as_any()
        .downcast_ref::<ListArray>()
        .ok_or_else(|| DbError::InvalidData(format!("column {column} is not list")))
}

fn list_string_value(array: &ListArray, row: usize) -> Vec<String> {
    if array.is_null(row) {
        return vec![];
    }
    let values = array.value(row);
    let Some(strings) = values.as_any().downcast_ref::<StringArray>() else {
        return vec![];
    };
    (0..strings.len())
        .map(|i| string_value(strings, i))
        .collect()
}

fn fixed_f32_column<'a>(batch: &'a RecordBatch, column: &str) -> DbResult<&'a FixedSizeListArray> {
    let idx = batch
        .schema()
        .index_of(column)
        .map_err(|_| DbError::MissingColumn(column.to_string()))?;
    batch
        .column(idx)
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .ok_or_else(|| DbError::InvalidData(format!("column {column} is not fixed-size list")))
}

fn fixed_f32_value(array: &FixedSizeListArray, row: usize) -> Vec<f32> {
    if array.is_null(row) {
        return vec![];
    }
    let values = array.value(row);
    let Some(floats) = values.as_any().downcast_ref::<Float32Array>() else {
        return vec![];
    };
    (0..floats.len())
        .map(|i| {
            if floats.is_null(i) {
                0.0
            } else {
                floats.value(i)
            }
        })
        .collect()
}

fn int_column<'a>(batch: &'a RecordBatch, column: &str) -> DbResult<&'a Int64Array> {
    let idx = batch
        .schema()
        .index_of(column)
        .map_err(|_| DbError::MissingColumn(column.to_string()))?;
    batch
        .column(idx)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| DbError::InvalidData(format!("column {column} is not int64")))
}

fn int_value(array: &Int64Array, row: usize) -> i64 {
    if array.is_null(row) {
        0
    } else {
        array.value(row)
    }
}

fn float_column<'a>(batch: &'a RecordBatch, column: &str) -> DbResult<&'a dyn Array> {
    let idx = batch
        .schema()
        .index_of(column)
        .map_err(|_| DbError::MissingColumn(column.to_string()))?;
    Ok(batch.column(idx).as_ref())
}

fn float_value(array: &dyn Array, row: usize) -> f64 {
    if let Some(values) = array.as_any().downcast_ref::<Float32Array>() {
        if values.is_null(row) {
            0.0
        } else {
            values.value(row) as f64
        }
    } else if let Some(values) = array.as_any().downcast_ref::<Float64Array>() {
        if values.is_null(row) {
            0.0
        } else {
            values.value(row)
        }
    } else {
        0.0
    }
}

fn enrich_taxonomy_tags(
    mut tags: Vec<TaxonomyTag>,
    counts: &HashMap<String, usize>,
) -> Vec<TaxonomyTag> {
    let tag_names = tags.iter().map(|tag| tag.tag.clone()).collect::<Vec<_>>();

    for tag in &mut tags {
        tag.broader = parent_tag(&tag.tag).into_iter().collect();
        tag.narrower = tag_names
            .iter()
            .filter(|candidate| parent_tag(candidate).as_deref() == Some(tag.tag.as_str()))
            .cloned()
            .collect();
        tag.narrower.sort();
        tag.count = counts.get(&tag.tag).copied().unwrap_or(0);
    }

    tags.sort_by(|a, b| a.tag.cmp(&b.tag));
    tags
}

fn parent_tag(tag: &str) -> Option<String> {
    let tag = normalize_tag(tag)?;
    let parts = tag.trim_start_matches('#').split('/').collect::<Vec<_>>();
    if parts.len() <= 1 {
        None
    } else {
        Some(format!("#{}", parts[..parts.len() - 1].join("/")))
    }
}

fn tags_contain(tags: &[String], target: &str) -> bool {
    tags.iter()
        .filter_map(|tag| normalize_tag(tag))
        .any(|tag| tag == target)
}

fn normalize_tag_required(tag: &str) -> DbResult<String> {
    normalize_tag(tag).ok_or_else(|| DbError::InvalidData("tag cannot be empty".to_string()))
}

fn validate_taxonomy_tag(tag: &str) -> DbResult<String> {
    let raw = tag.trim();
    if raw.is_empty() {
        return Err(DbError::InvalidData("tag cannot be empty".to_string()));
    }

    let without_hash = raw.strip_prefix('#').unwrap_or(raw);
    let parts = without_hash.split('/').collect::<Vec<_>>();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| part.is_empty() || part.chars().any(char::is_whitespace))
    {
        return Err(DbError::InvalidData(format!("invalid taxonomy tag: {tag}")));
    }

    Ok(format!("#{}", parts.join("/")))
}

fn is_tag_or_descendant(tag: &str, prefix: &str) -> bool {
    tag == prefix
        || tag
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn rename_tag_path(tag: &str, old_tag: &str, new_tag: &str) -> Option<String> {
    if tag == old_tag {
        return Some(new_tag.to_string());
    }
    tag.strip_prefix(old_tag)
        .filter(|rest| rest.starts_with('/'))
        .map(|rest| format!("{new_tag}{rest}"))
}

fn rename_tags_in_array(tags: Vec<String>, old_tag: &str, new_tag: &str) -> (Vec<String>, bool) {
    let mut changed = false;
    let mut renamed = BTreeSet::new();

    for tag in tags {
        let Some(normalized) = normalize_tag(&tag) else {
            changed = true;
            continue;
        };
        let next = rename_tag_path(&normalized, old_tag, new_tag).unwrap_or(normalized.clone());
        if next != tag || next != normalized {
            changed = true;
        }
        renamed.insert(next);
    }

    let out = renamed.into_iter().collect::<Vec<_>>();
    (out, changed)
}

fn merge_taxonomy_metadata(destination: TaxonomyTag, incoming: TaxonomyTag) -> TaxonomyTag {
    TaxonomyTag {
        tag: destination.tag,
        description: if destination.description.trim().is_empty() {
            incoming.description
        } else {
            destination.description
        },
        color: destination.color.or(incoming.color),
        broader: vec![],
        narrower: vec![],
        count: 0,
    }
}

fn normalize_tag(tag: &str) -> Option<String> {
    let tag = tag.trim();
    if tag.is_empty() {
        return None;
    }

    let tag = tag.trim_start_matches('#');
    let parts = tag
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        None
    } else {
        Some(format!("#{}", parts.join("/")))
    }
}

fn tag_with_ancestors(tag: &str) -> Vec<String> {
    let Some(normalized) = normalize_tag(tag) else {
        return vec![];
    };
    let parts = normalized
        .trim_start_matches('#')
        .split('/')
        .collect::<Vec<_>>();

    (1..=parts.len())
        .map(|end| format!("#{}", parts[..end].join("/")))
        .collect()
}

fn escape_sql(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::TryStreamExt;

    fn test_config() -> (tempfile::TempDir, DbConfig) {
        let dir = tempfile::tempdir().unwrap();
        let config = DbConfig {
            lance_path: dir.path().join("lance.journal-app"),
            journal_dir: None,
            evergreen_dir: None,
            embeddings_path: None,
            ingest_on_startup: false,
        };
        (dir, config)
    }

    async fn taxonomy_tags(db: &Db) -> BTreeSet<String> {
        let table = db.conn.open_table("taxonomy").execute().await.unwrap();
        let batches = table
            .query()
            .execute()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        taxonomy_tags_from_batches(&batches)
            .unwrap()
            .into_iter()
            .map(|tag| tag.tag)
            .collect()
    }

    #[tokio::test]
    async fn creates_threads_and_messages() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let names = db.conn.table_names().execute().await.unwrap();
        assert!(names.contains(&"threads".to_string()));
        assert!(names.contains(&"messages".to_string()));
        assert!(names.contains(&"log_events".to_string()));
        assert!(names.contains(&"taxonomy".to_string()));
    }

    #[tokio::test]
    async fn persists_thread_and_messages() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let thread = db
            .create_thread(Some("Test thread".to_string()), None)
            .await
            .unwrap();
        let message = db
            .save_message(
                thread.thread_id.clone(),
                "user".to_string(),
                "hello".to_string(),
                None,
            )
            .await
            .unwrap();

        let threads = db.get_threads().await.unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].title, "Test thread");

        let messages = db
            .get_thread_messages(thread.thread_id.clone())
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message_id, message.message_id);
        assert_eq!(messages[0].content, "hello");
    }

    #[tokio::test]
    async fn updates_and_deletes_thread() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let thread = db
            .create_thread(None, Some("initial".to_string()))
            .await
            .unwrap();
        db.update_thread_title(thread.thread_id.clone(), "Renamed".to_string())
            .await
            .unwrap();
        assert_eq!(
            db.get_thread(thread.thread_id.clone())
                .await
                .unwrap()
                .unwrap()
                .title,
            "Renamed"
        );

        db.delete_thread(thread.thread_id.clone()).await.unwrap();
        assert!(db
            .get_thread(thread.thread_id.clone())
            .await
            .unwrap()
            .is_none());
        assert!(db
            .get_thread_messages(thread.thread_id)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn creates_and_lists_log_events() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let first = db
            .create_log_event(LogEvent {
                log_event_id: "frontend-ignored".to_string(),
                datetime: "2024-01-01T10:00:00Z".to_string(),
                text: "first".to_string(),
                tags: vec!["a".to_string(), "b".to_string()],
            })
            .await
            .unwrap();
        let second = db
            .create_log_event(LogEvent {
                log_event_id: String::new(),
                datetime: "2024-01-02T10:00:00Z".to_string(),
                text: "second".to_string(),
                tags: vec!["a".to_string()],
            })
            .await
            .unwrap();

        assert!(!first.log_event_id.is_empty());
        assert_ne!(first.log_event_id, "frontend-ignored");
        assert!(!second.log_event_id.is_empty());

        let descending = db
            .list_log_events(Some("descending".to_string()), None, None)
            .await
            .unwrap();
        assert_eq!(descending.len(), 2);
        assert_eq!(descending[0].text, "second");
        assert_eq!(descending[1].text, "first");

        let ascending = db
            .list_log_events(Some("ascending".to_string()), Some(1), None)
            .await
            .unwrap();
        assert_eq!(ascending.len(), 1);
        assert_eq!(ascending[0].text, "first");

        let tagged = db
            .list_log_events(None, None, Some(vec!["a".to_string(), "b".to_string()]))
            .await
            .unwrap();
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].text, "first");
    }

    #[tokio::test]
    async fn sync_taxonomy_creates_normalized_tags_and_ancestors_from_logs() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        db.create_log_event(LogEvent {
            log_event_id: String::new(),
            datetime: "2024-01-01T10:00:00Z".to_string(),
            text: "played game".to_string(),
            tags: vec!["Video_Games/Nuclear_Option".to_string()],
        })
        .await
        .unwrap();

        let tags = taxonomy_tags(&db).await;
        assert!(tags.contains("#Video_Games"));
        assert!(tags.contains("#Video_Games/Nuclear_Option"));
    }

    #[tokio::test]
    async fn sync_taxonomy_preserves_existing_metadata() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let table = db.conn.open_table("taxonomy").execute().await.unwrap();
        table
            .add(
                taxonomy_batch(&[TaxonomyTag {
                    tag: "#Work".to_string(),
                    description: "work stuff".to_string(),
                    color: Some("#F54927".to_string()),
                    broader: vec![],
                    narrower: vec![],
                    count: 0,
                }])
                .unwrap(),
            )
            .execute()
            .await
            .unwrap();

        db.create_log_event(LogEvent {
            log_event_id: String::new(),
            datetime: "2024-01-01T10:00:00Z".to_string(),
            text: "worked on EK".to_string(),
            tags: vec!["#Work/EK".to_string()],
        })
        .await
        .unwrap();
        db.sync_taxonomy().await.unwrap();

        let table = db.conn.open_table("taxonomy").execute().await.unwrap();
        let batches = table
            .query()
            .execute()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        let tags = taxonomy_tags_from_batches(&batches).unwrap();
        let work = tags.iter().find(|tag| tag.tag == "#Work").unwrap();
        assert_eq!(work.description, "work stuff");
        assert_eq!(work.color.as_deref(), Some("#F54927"));
        assert!(tags.iter().any(|tag| tag.tag == "#Work/EK"));
    }

    #[tokio::test]
    async fn sync_taxonomy_collects_thread_tags_and_threads_from_batches_reads_tags() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let thread = Thread {
            thread_id: "thread-1".to_string(),
            title: "Tagged thread".to_string(),
            tags: Some(vec!["Work/EK".to_string()]),
            created_at: "2024-01-01T10:00:00Z".to_string(),
            updated_at: "2024-01-01T10:00:00Z".to_string(),
        };
        let table = db.conn.open_table("threads").execute().await.unwrap();
        table
            .add(thread_batch(&[thread]).unwrap())
            .execute()
            .await
            .unwrap();

        let stream = table.query().execute().await.unwrap();
        let batches = stream.try_collect::<Vec<_>>().await.unwrap();
        let threads = threads_from_batches(&batches).unwrap();
        assert_eq!(threads[0].tags, Some(vec!["Work/EK".to_string()]));

        db.sync_taxonomy().await.unwrap();
        let tags = taxonomy_tags(&db).await;
        assert!(tags.contains("#Work"));
        assert!(tags.contains("#Work/EK"));
    }

    #[tokio::test]
    async fn list_taxonomy_tags_returns_rich_tags_with_direct_children() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        db.create_log_event(LogEvent {
            log_event_id: String::new(),
            datetime: "2024-01-01T10:00:00Z".to_string(),
            text: "taxonomy shape".to_string(),
            tags: vec!["#A/B".to_string(), "#A/C/D".to_string()],
        })
        .await
        .unwrap();

        let tags = db.list_taxonomy_tags().await.unwrap();
        let get = |name: &str| tags.iter().find(|tag| tag.tag == name).unwrap();

        assert_eq!(get("#A").broader, Vec::<String>::new());
        assert_eq!(get("#A").narrower, vec!["#A/B", "#A/C"]);
        assert_eq!(get("#A/B").broader, vec!["#A"]);
        assert_eq!(get("#A/B").narrower, Vec::<String>::new());
        assert_eq!(get("#A/C").broader, vec!["#A"]);
        assert_eq!(get("#A/C").narrower, vec!["#A/C/D"]);
        assert_eq!(get("#A/C/D").broader, vec!["#A/C"]);
        assert_eq!(get("#A/C/D").narrower, Vec::<String>::new());
        assert_eq!(get("#A/B").count, 1);
        assert_eq!(get("#A").count, 0);
    }

    #[tokio::test]
    async fn update_taxonomy_tag_persists_description_and_nullable_color() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let updated = db
            .update_taxonomy_tag(UpdateTaxonomyTagRequest {
                tag: "Work/EK".to_string(),
                description: "EK work".to_string(),
                color: Some("#F54927".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(updated.tag, "#Work/EK");
        assert_eq!(updated.description, "EK work");
        assert_eq!(updated.color.as_deref(), Some("#F54927"));

        let cleared = db
            .update_taxonomy_tag(UpdateTaxonomyTagRequest {
                tag: "#Work/EK".to_string(),
                description: "EK work".to_string(),
                color: None,
            })
            .await
            .unwrap();
        assert_eq!(cleared.color, None);

        let read = db
            .get_taxonomy_tag("Work/EK".to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read.description, "EK work");
        assert_eq!(read.color, None);
    }

    #[tokio::test]
    async fn resolve_tag_colors_inherits_from_ancestors() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        db.update_taxonomy_tag(UpdateTaxonomyTagRequest {
            tag: "#Work".to_string(),
            description: "".to_string(),
            color: Some("#0000FF".to_string()),
        })
        .await
        .unwrap();
        db.update_taxonomy_tag(UpdateTaxonomyTagRequest {
            tag: "#Work/EK".to_string(),
            description: "".to_string(),
            color: None,
        })
        .await
        .unwrap();

        let colors = db
            .resolve_tag_colors(vec!["Work/EK/Deep".to_string(), "Other".to_string()])
            .await
            .unwrap();
        assert_eq!(
            colors.get("#Work/EK/Deep"),
            Some(&Some("#0000FF".to_string()))
        );
        assert_eq!(colors.get("#Other"), Some(&None));
    }

    #[tokio::test]
    async fn list_tag_instances_sorts_newest_first() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        for (datetime, text) in [
            ("2024-01-01T10:00:00Z", "first"),
            ("2024-01-03T10:00:00Z", "third"),
            ("2024-01-02T10:00:00Z", "second"),
        ] {
            db.create_log_event(LogEvent {
                log_event_id: String::new(),
                datetime: datetime.to_string(),
                text: text.to_string(),
                tags: vec!["A".to_string()],
            })
            .await
            .unwrap();
        }

        let instances = db.list_tag_instances("#A".to_string(), None).await.unwrap();
        assert_eq!(instances.len(), 3);
        assert_eq!(
            instances[0].datetime.as_deref(),
            Some("2024-01-03T10:00:00Z")
        );
        assert_eq!(
            instances[1].datetime.as_deref(),
            Some("2024-01-02T10:00:00Z")
        );
        assert_eq!(
            instances[2].datetime.as_deref(),
            Some("2024-01-01T10:00:00Z")
        );
        assert!(instances
            .iter()
            .all(|instance| instance.source_type == "log_event"));

        let limited = db
            .list_tag_instances("A".to_string(), Some(2))
            .await
            .unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[tokio::test]
    async fn rename_taxonomy_tag_moves_metadata() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        db.update_taxonomy_tag(UpdateTaxonomyTagRequest {
            tag: "Old".to_string(),
            description: "old metadata".to_string(),
            color: Some("#123456".to_string()),
        })
        .await
        .unwrap();

        let renamed = db
            .rename_taxonomy_tag("Old".to_string(), "New".to_string())
            .await
            .unwrap();
        assert_eq!(renamed.tag, "#New");
        assert_eq!(renamed.description, "old metadata");
        assert_eq!(renamed.color.as_deref(), Some("#123456"));
    }

    #[tokio::test]
    async fn rename_taxonomy_tag_rewrites_source_tags_descendants_and_dedupes() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let log = db
            .create_log_event(LogEvent {
                log_event_id: String::new(),
                datetime: "2024-01-01T10:00:00Z".to_string(),
                text: "rename me".to_string(),
                tags: vec![
                    "#Old".to_string(),
                    "#Old/Sub".to_string(),
                    "#New/Sub".to_string(),
                ],
            })
            .await
            .unwrap();

        let threads = db.conn.open_table("threads").execute().await.unwrap();
        threads
            .add(
                thread_batch(&[Thread {
                    thread_id: "thread-rename".to_string(),
                    title: "Tagged thread".to_string(),
                    tags: Some(vec!["Old/Thread".to_string()]),
                    created_at: "2024-01-01T10:00:00Z".to_string(),
                    updated_at: "2024-01-01T10:00:00Z".to_string(),
                }])
                .unwrap(),
            )
            .execute()
            .await
            .unwrap();

        db.conn
            .create_table(
                "journal",
                journal_batch(&[JournalRow {
                    entry_id: "journal-rename".to_string(),
                    date: "2024-01-01".to_string(),
                    title: "Journal rename".to_string(),
                    text: "journal text".to_string(),
                    tags: vec!["#Old/Journal".to_string()],
                    embedding: vec![1.0, 0.0],
                    entry_type: "daily".to_string(),
                    source_path: "/tmp/journal-rename.md".to_string(),
                    embedding_key: "/tmp/journal-rename.md".to_string(),
                    content_hash: "hash".to_string(),
                    source_mtime_ms: 0,
                    indexed_at: "2024-01-01T10:00:00Z".to_string(),
                }])
                .unwrap(),
            )
            .execute()
            .await
            .unwrap();

        db.rename_taxonomy_tag("Old".to_string(), "New".to_string())
            .await
            .unwrap();

        let log_events = db.list_log_events(None, None, None).await.unwrap();
        let renamed_log = log_events
            .iter()
            .find(|event| event.log_event_id == log.log_event_id)
            .unwrap();
        assert_eq!(renamed_log.tags, vec!["#New", "#New/Sub"]);

        let threads = db.conn.open_table("threads").execute().await.unwrap();
        let thread_batches = threads
            .query()
            .execute()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        let renamed_thread = threads_from_batches(&thread_batches)
            .unwrap()
            .into_iter()
            .find(|thread| thread.thread_id == "thread-rename")
            .unwrap();
        assert_eq!(renamed_thread.tags, Some(vec!["#New/Thread".to_string()]));

        let journal = db.conn.open_table("journal").execute().await.unwrap();
        let journal_batches = journal
            .query()
            .execute()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        let renamed_journal = journal_rows_from_batches(&journal_batches)
            .unwrap()
            .into_iter()
            .find(|row| row.entry_id == "journal-rename")
            .unwrap();
        assert_eq!(renamed_journal.tags, vec!["#New/Journal"]);

        let taxonomy = taxonomy_tags(&db).await;
        assert!(taxonomy.contains("#New"));
        assert!(taxonomy.contains("#New/Sub"));
        assert!(taxonomy.contains("#New/Thread"));
        assert!(taxonomy.contains("#New/Journal"));
        assert!(!taxonomy.iter().any(|tag| tag.starts_with("#Old")));
    }

    #[tokio::test]
    async fn rejects_empty_log_event_text() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let error = db
            .create_log_event(LogEvent {
                log_event_id: String::new(),
                datetime: "2024-01-01T10:00:00Z".to_string(),
                text: "   ".to_string(),
                tags: vec![],
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("text cannot be empty"));
    }

    #[tokio::test]
    async fn ingests_journal_incrementally_and_retrieves_recent() {
        let dir = tempfile::tempdir().unwrap();
        let journal_dir = dir.path().join("Daily Pages");
        std::fs::create_dir_all(&journal_dir).unwrap();
        std::fs::write(
            journal_dir.join("01-01-2024.md"),
            "# New Year #life\n### Transcription\nfirst entry\n### Other\nignored",
        )
        .unwrap();
        std::fs::write(
            journal_dir.join("01-02-2024.md"),
            "# Next #work\n### Transcription\nsecond entry",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("embeddings.jsonl"),
            r#"{"path":"/fake/01-01-2024.md","embedding":[1.0,0.0,0.0,0.0]}
{"path":"/fake/01-02-2024.md","embedding":[0.0,1.0,0.0,0.0]}
"#,
        )
        .unwrap();

        let config = DbConfig {
            lance_path: dir.path().join("lance.journal-app"),
            journal_dir: Some(journal_dir),
            evergreen_dir: None,
            embeddings_path: Some(dir.path().join("embeddings.jsonl")),
            ingest_on_startup: false,
        };
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();
        db.startup_ingest().await.unwrap();

        let recent = db.get_recent_entries(2).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].date, "2024-01-02");
        assert_eq!(recent[0].text, "second entry");
        assert_eq!(recent[1].tags, vec!["#life"]);

        let similar = db
            .get_similar_entries(vec![0.0, 1.0, 0.0, 0.0], 1)
            .await
            .unwrap();
        assert_eq!(similar.len(), 1);
        assert_eq!(similar[0].0.date, "2024-01-02");

        let taxonomy = taxonomy_tags(&db).await;
        assert!(taxonomy.contains("#life"));
        assert!(taxonomy.contains("#work"));
    }
}
