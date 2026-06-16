use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub thread_id: String,
    pub title: String,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_id: String,
    pub thread_id: String,
    pub timestamp: String,
    pub role: String,
    pub content: String,
    pub metadata: Option<MessageMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageModelMetadata {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePersonalityMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContextEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
    pub date: Option<String>,
    pub title: String,
    pub entry_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub tags: Vec<String>,
    pub distance: Option<f64>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContextChat {
    pub thread_id: String,
    pub message_id: Option<String>,
    pub role: Option<String>,
    pub content: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchIteration {
    pub iteration: i32,
    pub tool: String,
    pub reasoning: String,
    pub query: Option<String>,
    pub results_count: i32,
    pub new_entries_added: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    pub model: MessageModelMetadata,
    pub personality: Option<MessagePersonalityMetadata>,
    pub context_entries: Vec<MessageContextEntry>,
    pub context_chats: Vec<MessageContextChat>,
    pub retrieval_trace: Vec<SearchIteration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub entry_id: String,
    pub date: String,
    pub title: String,
    pub text: String,
    pub tags: Vec<String>,
    pub embedding: Option<Vec<f64>>,
    pub entry_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    #[serde(default)]
    pub log_event_id: String,
    pub datetime: String,
    pub text: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSummary {
    pub tag: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct TaxonomyTag {
    pub tag: String,
    pub description: String,
    pub color: Option<String>,
    #[serde(default)]
    pub broader: Vec<String>,
    #[serde(default)]
    pub narrower: Vec<String>,
    #[serde(default)]
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct UpdateTaxonomyTagRequest {
    pub tag: String,
    pub description: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedDoc {
    pub entry: Entry,
    pub distance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub query: String,
    pub top_k: Option<i32>,
    pub provider: String,
    pub model: String,
    pub thread_id: Option<String>,
    pub message_history: Option<Vec<serde_json::Value>>,
    pub existing_docs: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub response: String,
    pub docs: Vec<RetrievedDoc>,
    pub thread_id: Option<String>,
    pub message_metadata: Option<MessageMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateThreadRequest {
    pub title: Option<String>,
    pub initial_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateThreadResponse {
    pub thread_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddMessageRequest {
    pub role: String,
    pub content: String,
    pub metadata: Option<MessageMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateThreadRequest {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum StreamEvent {
    SearchIteration(SearchIteration),
    ChatResponse(ChatResponse),
    Error { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateThreadTitleResponse {
    pub title: String,
}
