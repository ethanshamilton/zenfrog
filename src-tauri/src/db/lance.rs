use std::{path::PathBuf, sync::Arc};

use arrow_array::{
    builder::{ListBuilder, StringBuilder},
    Array, ArrayRef, RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use futures::TryStreamExt;
use lancedb::{
    database::CreateTableMode,
    query::{ExecutableQuery, QueryBase, Select},
    Connection,
};
use uuid::Uuid;

use crate::models::{Message, MessageMetadata, Thread};

use super::{DbError, DbResult};

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
        Ok(())
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
        table.add(thread_batch(&[thread.clone()])?).execute().await?;

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
            return Err(DbError::InvalidData(format!("thread not found: {thread_id}")));
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
        table.add(message_batch(&[message.clone()])?).execute().await?;

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

    async fn table_exists(&self, name: &str) -> DbResult<bool> {
        let names = self.conn.table_names().execute().await?;
        Ok(names.iter().any(|table| table == name))
    }
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
                threads.iter().map(|t| t.thread_id.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                threads.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            string_list_array(&tags),
            Arc::new(StringArray::from(
                threads.iter().map(|t| t.created_at.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                threads.iter().map(|t| t.updated_at.as_str()).collect::<Vec<_>>(),
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
                messages.iter().map(|m| m.message_id.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                messages.iter().map(|m| m.thread_id.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                messages.iter().map(|m| m.timestamp.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
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
                Some(serde_json::from_str::<MessageMetadata>(metadata_json.value(i))?)
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

        let messages = db.get_thread_messages(thread.thread_id.clone()).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message_id, message.message_id);
        assert_eq!(messages[0].content, "hello");
    }

    #[tokio::test]
    async fn updates_and_deletes_thread() {
        let (_dir, config) = test_config();
        let db = Db::connect(config).await.unwrap();
        db.startup_ingest().await.unwrap();

        let thread = db.create_thread(None, Some("initial".to_string())).await.unwrap();
        db.update_thread_title(thread.thread_id.clone(), "Renamed".to_string())
            .await
            .unwrap();
        assert_eq!(
            db.get_thread(thread.thread_id.clone()).await.unwrap().unwrap().title,
            "Renamed"
        );

        db.delete_thread(thread.thread_id.clone()).await.unwrap();
        assert!(db.get_thread(thread.thread_id.clone()).await.unwrap().is_none());
        assert!(db.get_thread_messages(thread.thread_id).await.unwrap().is_empty());
    }
}
