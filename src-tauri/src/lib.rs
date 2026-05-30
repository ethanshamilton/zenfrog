mod models;

use std::sync::Mutex;

use models::*;
use tauri::{ipc::Channel, Manager, State};

struct AppStatus {
    status: String,
}

fn mock_timestamp() -> String {
    "2026-05-30T00:00:00Z".to_string()
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
async fn create_thread(req: CreateThreadRequest) -> Result<CreateThreadResponse, String> {
    let _ = req;
    Ok(CreateThreadResponse {
        thread_id: "mock-thread-id".to_string(),
        created_at: mock_timestamp(),
    })
}

#[tauri::command]
async fn get_threads() -> Result<Vec<Thread>, String> {
    Ok(vec![])
}

#[tauri::command]
async fn get_thread(thread_id: String) -> Result<Thread, String> {
    let _ = thread_id;
    Err("not implemented".into())
}

#[tauri::command]
async fn get_thread_messages(thread_id: String) -> Result<Vec<Message>, String> {
    let _ = thread_id;
    Ok(vec![])
}

#[tauri::command]
async fn add_message_to_thread(
    thread_id: String,
    req: AddMessageRequest,
) -> Result<Message, String> {
    Ok(Message {
        message_id: "mock-message-id".to_string(),
        thread_id,
        timestamp: mock_timestamp(),
        role: req.role,
        content: req.content,
        metadata: req.metadata,
    })
}

#[tauri::command]
async fn update_thread_title(thread_id: String, req: UpdateThreadRequest) -> Result<(), String> {
    let _ = (thread_id, req);
    Err("not implemented".into())
}

#[tauri::command]
async fn generate_thread_title(thread_id: String) -> Result<GenerateThreadTitleResponse, String> {
    let _ = thread_id;
    Ok(GenerateThreadTitleResponse {
        title: "Mock thread title".to_string(),
    })
}

#[tauri::command]
async fn delete_thread(thread_id: String) -> Result<(), String> {
    let _ = thread_id;
    Err("not implemented".into())
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

            // TODO: manage Db handle here once backend storage is ported.

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
