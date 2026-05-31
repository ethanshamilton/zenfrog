mod db;
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
        journal_dir: None,
        evergreen_dir: None,
        embeddings_path: None,
        chats_path: None,
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
async fn journal_chat(req: ChatRequest) -> Result<ChatResponse, String> {
    Ok(ChatResponse {
        response: "This is a mocked Tauri chat response.".to_string(),
        docs: vec![],
        thread_id: req.thread_id,
        message_metadata: None,
    })
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
async fn generate_thread_title(thread_id: String) -> Result<GenerateThreadTitleResponse, String> {
    let _ = thread_id;
    Ok(GenerateThreadTitleResponse {
        title: "Mock thread title".to_string(),
    })
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
    req: ChatRequest,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    on_event
        .send(StreamEvent::SearchIteration(SearchIteration {
            iteration: 1,
            tool: "mock_search".to_string(),
            reasoning: "Validating Tauri channel wiring with a fake search iteration.".to_string(),
            query: Some(req.query.clone()),
            results_count: 0,
            new_entries_added: 0,
        }))
        .map_err(|err| err.to_string())?;

    on_event
        .send(StreamEvent::ChatResponse(ChatResponse {
            response: "This is a mocked Tauri chat response.".to_string(),
            docs: vec![],
            thread_id: req.thread_id,
            message_metadata: None,
        }))
        .map_err(|err| err.to_string())?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
