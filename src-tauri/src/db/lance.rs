use std::{collections::HashMap, path::PathBuf, sync::Arc};

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

use crate::models::{Entry, Message, MessageMetadata, Thread};

use super::{ingest, ingest::JournalRow, DbError, DbResult};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DbConfig {
    pub lance_path: PathBuf,
    pub journal_dir: Option<PathBuf>,
    pub evergreen_dir: Option<PathBuf>,
    pub embeddings_path: Option<PathBuf>,
    pub chats_path: Option<PathBuf>,
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
        self.incremental_journal_ingest().await?;
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
            metadata,
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

fn threads_from_batches(batches: &[RecordBatch]) -> DbResult<Vec<Thread>> {
    let mut threads = Vec::new();
    for batch in batches {
        let thread_ids = string_column(batch, "thread_id")?;
        let titles = string_column(batch, "title")?;
        let created = string_column(batch, "created_at")?;
        let updated = string_column(batch, "updated_at")?;

        for i in 0..batch.num_rows() {
            threads.push(Thread {
                thread_id: string_value(thread_ids, i),
                title: string_value(titles, i),
                tags: Some(vec![]),
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

fn entries_from_batches(
    batches: &[RecordBatch],
    include_distance: bool,
) -> DbResult<Vec<(Entry, Option<f64>)>> {
    let mut entries = Vec::new();
    for batch in batches {
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

fn escape_sql(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> (tempfile::TempDir, DbConfig) {
        let dir = tempfile::tempdir().unwrap();
        let config = DbConfig {
            lance_path: dir.path().join("lance.journal-app"),
            journal_dir: None,
            evergreen_dir: None,
            embeddings_path: None,
            chats_path: None,
        };
        (dir, config)
    }

    #[tokio::test]
    async fn creates_threads_and_messages() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let names = db.conn.table_names().execute().await.unwrap();
        assert!(names.contains(&"threads".to_string()));
        assert!(names.contains(&"messages".to_string()));
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
            chats_path: None,
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
    }
}
