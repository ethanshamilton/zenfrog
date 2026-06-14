mod ai;
mod baml_client;
mod db;
mod ingestion;
mod llm;
mod models;

use std::{path::PathBuf, sync::Mutex};

use db::{Db, DbConfig};
use models::*;
use tauri::{ipc::Channel, Manager, State};

struct AppStatus {
    status: String,
}

struct DbState {
    db: Db,
}

fn load_dotenv() {
    let cwd = std::env::current_dir().ok();
    let candidates = [
        cwd.as_ref().map(|dir| dir.join(".env")),
        cwd.as_ref().map(|dir| dir.join("src-tauri/.env")),
        cwd.as_ref()
            .and_then(|dir| dir.parent().map(|parent| parent.join(".env"))),
    ];

    for path in candidates.into_iter().flatten() {
        if path.exists() {
            let _ = dotenvy::from_path(path);
            return;
        }
    }

    let _ = dotenvy::dotenv();
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn resolve_db_config(app: &tauri::App) -> Result<DbConfig, String> {
    let lance_path: PathBuf = if cfg!(debug_assertions) {
        std::env::current_dir()
            .map_err(|err| err.to_string())?
            .join(".data")
            .join("lance.journal-app")
    } else {
        app.path()
            .app_data_dir()
            .map_err(|err| err.to_string())?
            .join("lance.journal-app")
    };

    Ok(DbConfig {
        lance_path,
        journal_dir: env_path("ZENFROG_JOURNAL_DIR"),
        evergreen_dir: env_path("ZENFROG_EVERGREEN_DIR"),
        embeddings_path: env_path("ZENFROG_EMBEDDINGS_PATH"),
        ingest_on_startup: std::env::var("ZENFROG_INGEST_ON_STARTUP")
            .map(|value| value != "false" && value != "0")
            .unwrap_or(true),
    })
}

#[tauri::command]
async fn get_status(status: State<'_, Mutex<AppStatus>>) -> Result<StatusResponse, String> {
    let status = status.lock().map_err(|err| err.to_string())?;
    Ok(StatusResponse {
        status: status.status.clone(),
    })
}

#[tauri::command]
async fn journal_chat(state: State<'_, DbState>, req: ChatRequest) -> Result<ChatResponse, String> {
    llm::direct_chat(&state.db, req).await
}

#[tauri::command]
async fn get_recent_entries(
    state: State<'_, DbState>,
    n: Option<usize>,
) -> Result<Vec<Entry>, String> {
    state
        .db
        .get_recent_entries(n.unwrap_or(7))
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn vector_search_entries(
    state: State<'_, DbState>,
    embedding: Vec<f64>,
    n: Option<usize>,
) -> Result<Vec<RetrievedDoc>, String> {
    let results = state
        .db
        .get_similar_entries(embedding, n.unwrap_or(5))
        .await
        .map_err(|err| err.to_string())?;
    Ok(results
        .into_iter()
        .map(|(entry, distance)| RetrievedDoc {
            entry: Entry {
                embedding: None,
                ..entry
            },
            distance: Some(distance),
        })
        .collect())
}

#[tauri::command]
async fn get_entries_by_date_range(
    state: State<'_, DbState>,
    start_date: String,
    end_date: String,
    n: Option<usize>,
) -> Result<Vec<Entry>, String> {
    state
        .db
        .get_entries_by_date_range(start_date, end_date, n)
        .await
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| Entry {
                    embedding: None,
                    ..entry
                })
                .collect()
        })
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn create_thread(
    state: State<'_, DbState>,
    req: CreateThreadRequest,
) -> Result<CreateThreadResponse, String> {
    let thread = state
        .db
        .create_thread(req.title, req.initial_message)
        .await
        .map_err(|err| err.to_string())?;
    Ok(CreateThreadResponse {
        thread_id: thread.thread_id,
        created_at: thread.created_at,
    })
}

#[tauri::command]
async fn get_threads(state: State<'_, DbState>) -> Result<Vec<Thread>, String> {
    state.db.get_threads().await.map_err(|err| err.to_string())
}

#[tauri::command]
async fn get_thread(state: State<'_, DbState>, thread_id: String) -> Result<Thread, String> {
    state
        .db
        .get_thread(thread_id.clone())
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("thread not found: {thread_id}"))
}

#[tauri::command]
async fn get_thread_messages(
    state: State<'_, DbState>,
    thread_id: String,
) -> Result<Vec<Message>, String> {
    state
        .db
        .get_thread_messages(thread_id)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn add_message_to_thread(
    state: State<'_, DbState>,
    thread_id: String,
    req: AddMessageRequest,
) -> Result<Message, String> {
    state
        .db
        .save_message(thread_id, req.role, req.content, req.metadata)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn update_thread_title(
    state: State<'_, DbState>,
    thread_id: String,
    req: UpdateThreadRequest,
) -> Result<(), String> {
    state
        .db
        .update_thread_title(thread_id, req.title)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn generate_thread_title(
    state: State<'_, DbState>,
    thread_id: String,
) -> Result<GenerateThreadTitleResponse, String> {
    let messages = state
        .db
        .get_thread_messages(thread_id)
        .await
        .map_err(|err| err.to_string())?;
    let title = llm::generate_thread_title(&messages).await?;
    Ok(GenerateThreadTitleResponse { title })
}

#[tauri::command]
async fn delete_thread(state: State<'_, DbState>, thread_id: String) -> Result<(), String> {
    state
        .db
        .delete_thread(thread_id)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn journal_chat_agent_stream(
    state: State<'_, DbState>,
    req: ChatRequest,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    let response = llm::agent_chat(
        |step| {
            on_event
                .send(StreamEvent::SearchIteration(step))
                .map_err(|err| err.to_string())
        },
        &state.db,
        req,
    )
    .await;

    match response {
        Ok(response) => on_event
            .send(StreamEvent::ChatResponse(response))
            .map_err(|err| err.to_string()),
        Err(error) => {
            let _ = on_event.send(StreamEvent::Error {
                error: error.clone(),
            });
            Err(error)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_dotenv();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(Mutex::new(AppStatus {
                status: "starting".to_string(),
            }));

            let config = resolve_db_config(app)?;
            let db = tauri::async_runtime::block_on(async move {
                let db = Db::connect(config).await?;
                db.startup_ingest().await?;
                Ok::<Db, db::DbError>(db)
            })
            .map_err(|err| err.to_string())?;
            app.manage(DbState { db });

            if let Ok(mut status) = app.state::<Mutex<AppStatus>>().lock() {
                status.status = "ready".to_string();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            journal_chat,
            get_recent_entries,
            vector_search_entries,
            get_entries_by_date_range,
            create_thread,
            get_threads,
            get_thread,
            get_thread_messages,
            add_message_to_thread,
            update_thread_title,
            generate_thread_title,
            delete_thread,
            journal_chat_agent_stream,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
