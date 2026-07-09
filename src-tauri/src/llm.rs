use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    ai,
    baml_client::{
        async_client::B,
        types::{SearchOptions as BamlSearchOptions, SearchToolCall, SearchToolType},
    },
    db::Db,
    models::{
        ChatRequest, ChatResponse, Entry, LogEvent, Message, MessageContextEntry,
        MessageContextLogEvent, MessageMetadata, MessageModelMetadata, MessagePersonalityMetadata,
        RetrievedDoc, SearchIteration,
    },
};

const MAX_AGENT_ITERATIONS: i64 = 5;
const RECENT_PRESEED_COUNT: usize = 4;
const DEFAULT_LIMIT: usize = 5;
const MAX_CONTEXT_CHARS: usize = 48_000;

#[derive(Debug, Clone)]
enum ContextItem {
    Entry {
        entry: Entry,
        distance: Option<f64>,
        source: String,
    },
    LogEvent {
        event: LogEvent,
        source: String,
    },
}

#[derive(Debug, Clone)]
struct Personality {
    title: String,
    description: String,
    prompt: String,
}

#[derive(Debug, Default)]
struct AgentSearchState {
    items: HashMap<String, ContextItem>,
    ordered_keys: Vec<String>,
    search_trace: Vec<SearchIteration>,
}

impl AgentSearchState {
    fn key(item: &ContextItem) -> String {
        match item {
            ContextItem::Entry { entry, .. } if !entry.entry_id.is_empty() => {
                format!("entry:{}:{}", entry.entry_type, entry.entry_id)
            }
            ContextItem::Entry { entry, .. } => {
                format!("entry:{}:{}:{}", entry.entry_type, entry.date, entry.title)
            }
            ContextItem::LogEvent { event, .. } => format!("log_event:{}", event.log_event_id),
        }
    }

    fn add_item(&mut self, item: ContextItem) -> bool {
        let key = Self::key(&item);
        if self.items.contains_key(&key) {
            return false;
        }
        self.ordered_keys.push(key.clone());
        self.items.insert(key, item);
        true
    }

    fn add_items<I>(&mut self, items: I) -> usize
    where
        I: IntoIterator<Item = ContextItem>,
    {
        items.into_iter().filter(|item| self.add_item(item.clone())).count()
    }

    fn record_iteration(
        &mut self,
        iteration: i32,
        tool: impl Into<String>,
        reasoning: impl Into<String>,
        query: Option<String>,
        results_count: i32,
        new_entries_added: i32,
    ) -> SearchIteration {
        let step = SearchIteration {
            iteration,
            tool: tool.into(),
            reasoning: reasoning.into(),
            query,
            results_count,
            new_entries_added,
        };
        self.search_trace.push(step.clone());
        step
    }

    fn context_entries(&self) -> Vec<MessageContextEntry> {
        self.ordered_keys
            .iter()
            .filter_map(|key| match self.items.get(key) {
                Some(ContextItem::Entry {
                    entry,
                    distance,
                    source,
                }) => Some(entry_to_context(entry, *distance, source)),
                _ => None,
            })
            .collect()
    }

    fn context_logs(&self) -> Vec<MessageContextLogEvent> {
        self.ordered_keys
            .iter()
            .filter_map(|key| match self.items.get(key) {
                Some(ContextItem::LogEvent { event, source }) => {
                    Some(log_event_to_context(event, source))
                }
                _ => None,
            })
            .collect()
    }

    fn context_string(&self) -> String {
        let mut rendered = Vec::new();
        let mut total = 0usize;

        for (idx, key) in self.ordered_keys.iter().enumerate() {
            let Some(item) = self.items.get(key) else {
                continue;
            };
            let block = match item {
                ContextItem::Entry {
                    entry,
                    distance,
                    source,
                } => format!(
                    "<JOURNAL_ENTRY index=\"{}\" id=\"{}\" date=\"{}\" title=\"{}\" type=\"{}\" source=\"{}\" distance=\"{}\">\nTags: {}\n{}\n</JOURNAL_ENTRY>",
                    idx + 1,
                    entry.entry_id,
                    entry.date,
                    entry.title,
                    entry.entry_type,
                    source,
                    distance.map(|d| d.to_string()).unwrap_or_default(),
                    entry.tags.join(", "),
                    entry.text
                ),
                ContextItem::LogEvent { event, source } => format!(
                    "<LOG_EVENT index=\"{}\" id=\"{}\" datetime=\"{}\" source=\"{}\">\nTags: {}\n{}\n</LOG_EVENT>",
                    idx + 1,
                    event.log_event_id,
                    event.datetime,
                    source,
                    event.tags.join(", "),
                    event.text
                ),
            };
            total += block.len();
            if total > MAX_CONTEXT_CHARS {
                rendered.push("<TRUNCATED>Additional retrieved context omitted to stay within context limits.</TRUNCATED>".to_string());
                break;
            }
            rendered.push(block);
        }

        rendered.join("\n\n")
    }

    fn trace_string(&self) -> String {
        trace_to_string(&self.search_trace)
    }
}

fn custom_instructions_path() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    [
        cwd.join("CUSTOM_INSTRUCTIONS.md"),
        cwd.join("..").join("CUSTOM_INSTRUCTIONS.md"),
        cwd.join("..").join("..").join("CUSTOM_INSTRUCTIONS.md"),
    ]
    .into_iter()
    .find(|path| path.exists())
}

pub fn load_custom_instructions() -> String {
    custom_instructions_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn personality_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ZENFROG_PERSONALITY_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Some(path);
    }

    std::env::var_os("ZENFROG_JOURNAL_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .and_then(|path| path.parent().map(|parent| parent.join("Personalities")))
}

fn split_frontmatter(text: &str) -> (HashMap<String, String>, String) {
    if !text.starts_with("---") {
        return (HashMap::new(), text.to_string());
    }

    let mut lines = text.lines();
    let _ = lines.next();
    let mut frontmatter = HashMap::new();
    let mut body_lines = Vec::new();
    let mut in_frontmatter = true;

    for line in lines {
        if in_frontmatter && line.trim() == "---" {
            in_frontmatter = false;
            continue;
        }

        if in_frontmatter {
            if let Some((key, value)) = line.split_once(':') {
                frontmatter.insert(
                    key.trim().to_string(),
                    value.trim().trim_matches(['\"', '\'']).to_string(),
                );
            }
        } else {
            body_lines.push(line);
        }
    }

    if in_frontmatter {
        (HashMap::new(), text.to_string())
    } else {
        (frontmatter, body_lines.join("\n"))
    }
}

fn load_personalities() -> Vec<Personality> {
    let Some(dir) = personality_dir() else {
        return vec![];
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        eprintln!(
            "[llm] personality directory not found/readable: {}",
            dir.display()
        );
        return vec![];
    };

    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    paths.sort();

    paths
        .into_iter()
        .filter_map(|path| load_personality_file(&path))
        .collect()
}

fn load_personality_file(path: &Path) -> Option<Personality> {
    let content = fs::read_to_string(path)
        .map_err(|err| {
            eprintln!("[llm] failed to read personality {}: {err}", path.display());
            err
        })
        .ok()?;
    let (frontmatter, body) = split_frontmatter(&content);
    let description = frontmatter.get("description")?.trim().to_string();
    let prompt = body.trim().to_string();

    if description.is_empty() || prompt.is_empty() {
        eprintln!(
            "[llm] skipping personality missing description or body: {}",
            path.display()
        );
        return None;
    }

    Some(Personality {
        title: path.file_stem()?.to_string_lossy().to_string(),
        description,
        prompt,
    })
}

async fn classify_personality(query: &str) -> Option<Personality> {
    let personalities = load_personalities();
    if personalities.is_empty() {
        return None;
    }

    let options = personalities
        .iter()
        .map(|p| format!("- {}: {}", p.title, p.description))
        .collect::<Vec<_>>()
        .join("\n");

    let selected = B
        .PersonalityClassifier
        .call(query, options.as_str())
        .await
        .ok()?
        .trim()
        .to_string();

    personalities
        .into_iter()
        .find(|p| p.title.eq_ignore_ascii_case(&selected))
}

fn selected_client(req: &ChatRequest) -> Option<String> {
    if req.provider.trim().is_empty() || req.model.trim().is_empty() {
        None
    } else {
        Some(format!("{}/{}", req.provider.trim(), req.model.trim()))
    }
}

fn is_openrouter_request(req: &ChatRequest) -> bool {
    req.provider.trim().eq_ignore_ascii_case("openrouter")
}

fn openrouter_client_registry(req: &ChatRequest) -> Result<baml::ClientRegistry, String> {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| "OPENROUTER_API_KEY is required for OpenRouter models".to_string())?;
    let app_url = std::env::var("ZENFROG_OPENROUTER_APP_URL")
        .unwrap_or_else(|_| "https://zenfrog.local".to_string());
    let app_title = std::env::var("ZENFROG_OPENROUTER_APP_TITLE")
        .unwrap_or_else(|_| "Zenfrog".to_string());
    let app_categories = std::env::var("ZENFROG_OPENROUTER_APP_CATEGORIES")
        .unwrap_or_else(|_| "personal-agent,writing-assistant".to_string());

    let mut registry = baml::ClientRegistry::new();
    registry.add_llm_client(
        "ZenfrogOpenRouter",
        "openrouter",
        HashMap::from([
            ("model".to_string(), serde_json::json!(req.model.trim())),
            ("api_key".to_string(), serde_json::json!(api_key)),
            (
                "headers".to_string(),
                serde_json::json!({
                    "HTTP-Referer": app_url,
                    "X-OpenRouter-Title": app_title,
                    "X-OpenRouter-Categories": app_categories,
                }),
            ),
        ]),
    );
    registry.set_primary_client("ZenfrogOpenRouter");
    Ok(registry)
}

fn format_message_values(values: &[serde_json::Value]) -> Vec<String> {
    values
        .iter()
        .map(|msg| {
            let role = msg
                .get("role")
                .or_else(|| msg.get("sender"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_uppercase();
            let content = msg
                .get("content")
                .or_else(|| msg.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            format!("[{role}]: {content}")
        })
        .collect()
}

fn format_messages(messages: &[Message], request_history: Option<&[serde_json::Value]>) -> String {
    let mut lines = messages
        .iter()
        .map(|msg| format!("[{}]: {}", msg.role.to_uppercase(), msg.content))
        .collect::<Vec<_>>();

    if let Some(history) = request_history {
        lines.extend(format_message_values(history));
    }

    lines.join("\n\n")
}

fn entry_to_context(entry: &Entry, distance: Option<f64>, source: &str) -> MessageContextEntry {
    MessageContextEntry {
        entry_id: if entry.entry_id.is_empty() {
            None
        } else {
            Some(entry.entry_id.clone())
        },
        date: Some(entry.date.clone()),
        title: entry.title.clone(),
        entry_type: entry.entry_type.clone(),
        text: Some(entry.text.clone()),
        tags: entry.tags.clone(),
        distance,
        source: source.to_string(),
    }
}

fn log_event_to_context(event: &LogEvent, source: &str) -> MessageContextLogEvent {
    MessageContextLogEvent {
        log_event_id: event.log_event_id.clone(),
        datetime: event.datetime.clone(),
        text: Some(event.text.clone()),
        tags: event.tags.clone(),
        source: source.to_string(),
    }
}

fn format_entries(entries: &[MessageContextEntry]) -> String {
    entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            format!(
                "<ENTRY index=\"{}\" date=\"{}\" title=\"{}\" type=\"{}\" source=\"{}\">\nTags: {}\n{}\n</ENTRY>",
                idx + 1,
                entry.date.as_deref().unwrap_or(""),
                entry.title,
                entry.entry_type,
                entry.source,
                entry.tags.join(", "),
                entry.text.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn docs_from_context(entries: &[MessageContextEntry]) -> Vec<RetrievedDoc> {
    entries
        .iter()
        .map(|ctx| RetrievedDoc {
            entry: Entry {
                entry_id: ctx.entry_id.clone().unwrap_or_default(),
                date: ctx.date.clone().unwrap_or_default(),
                title: ctx.title.clone(),
                text: ctx.text.clone().unwrap_or_default(),
                tags: ctx.tags.clone(),
                embedding: None,
                entry_type: ctx.entry_type.clone(),
            },
            distance: ctx.distance,
        })
        .collect()
}

async fn load_chat_history(db: &Db, req: &ChatRequest) -> Result<Vec<Message>, String> {
    match &req.thread_id {
        Some(thread_id) if !thread_id.trim().is_empty() => db
            .get_thread_messages(thread_id.clone())
            .await
            .map_err(|err| err.to_string()),
        _ => Ok(vec![]),
    }
}

async fn initial_context(
    db: &Db,
    req: &ChatRequest,
) -> Result<(Vec<MessageContextEntry>, Vec<SearchIteration>), String> {
    let mut context = Vec::new();
    let mut trace = Vec::new();

    if let Some(values) = &req.existing_docs {
        for value in values {
            let maybe_entry = value
                .get("entry")
                .cloned()
                .or_else(|| Some(value.clone()))
                .and_then(|v| serde_json::from_value::<Entry>(v).ok());
            if let Some(entry) = maybe_entry {
                let distance = value.get("distance").and_then(|v| v.as_f64());
                context.push(entry_to_context(&entry, distance, "existing_docs"));
            }
        }

        if !context.is_empty() {
            trace.push(SearchIteration {
                iteration: 0,
                tool: "EXISTING_DOCS".to_string(),
                reasoning: "Reused relevant documents supplied by the frontend.".to_string(),
                query: Some(req.query.clone()),
                results_count: context.len() as i32,
                new_entries_added: context.len() as i32,
            });
            return Ok((context, trace));
        }
    }

    let search_mode = B
        .IntentClassifier
        .call(req.query.as_str())
        .await
        .unwrap_or(BamlSearchOptions::RECENT);

    match search_mode {
        BamlSearchOptions::VECTOR => {
            let limit = req.top_k.unwrap_or(DEFAULT_LIMIT as i32).max(1) as usize;
            match ai::embed_text(&req.query).await {
                Ok(embedding) => {
                    let results = db
                        .get_similar_entries(embedding, limit)
                        .await
                        .map_err(|err| err.to_string())?;
                    for (entry, distance) in &results {
                        context.push(entry_to_context(entry, Some(*distance), "vector_search"));
                    }
                    trace.push(SearchIteration {
                        iteration: 0,
                        tool: "VECTOR".to_string(),
                        reasoning: "Intent classifier selected vector search.".to_string(),
                        query: Some(req.query.clone()),
                        results_count: results.len() as i32,
                        new_entries_added: results.len() as i32,
                    });
                }
                Err(err) => {
                    eprintln!(
                        "[llm] vector search embedding failed, falling back to recent: {err}"
                    );
                }
            }
        }
        BamlSearchOptions::RECENT => {}
    }

    if context.is_empty() {
        let limit = req.top_k.unwrap_or(7).max(1) as usize;
        let entries = db
            .get_recent_entries(limit)
            .await
            .map_err(|err| err.to_string())?;
        context.extend(
            entries
                .iter()
                .map(|entry| entry_to_context(entry, None, "recent_entries")),
        );
        trace.push(SearchIteration {
            iteration: 0,
            tool: "RECENT".to_string(),
            reasoning: "Intent classifier selected recent-entry retrieval or vector search was unavailable.".to_string(),
            query: Some(req.query.clone()),
            results_count: context.len() as i32,
            new_entries_added: context.len() as i32,
        });
    }

    Ok((context, trace))
}

fn response_metadata(
    req: &ChatRequest,
    personality: Option<&Personality>,
    context_entries: Vec<MessageContextEntry>,
    context_logs: Vec<MessageContextLogEvent>,
    retrieval_trace: Vec<SearchIteration>,
) -> MessageMetadata {
    MessageMetadata {
        model: MessageModelMetadata {
            provider: req.provider.clone(),
            model: req.model.clone(),
        },
        personality: personality.map(|p| MessagePersonalityMetadata {
            title: Some(p.title.clone()),
            description: Some(p.description.clone()),
            prompt: Some(p.prompt.clone()),
        }),
        context_entries,
        context_logs,
        context_chats: vec![],
        retrieval_trace,
    }
}

pub async fn generate_thread_title(messages: &[Message]) -> Result<String, String> {
    if messages.is_empty() {
        return Ok("Untitled".to_string());
    }

    let formatted = messages
        .iter()
        .map(|m| format!("[{}]: {}", m.role.to_uppercase(), m.content))
        .collect::<Vec<_>>()
        .join("\n\n");
    let title = B
        .GenerateThreadTitle
        .call(formatted.as_str())
        .await
        .map_err(|err| err.to_string())?
        .trim()
        .trim_matches(['\"', '\''])
        .to_string();

    Ok(if title.is_empty() {
        "Untitled".to_string()
    } else {
        title
    })
}

pub async fn direct_chat(db: &Db, req: ChatRequest) -> Result<ChatResponse, String> {
    let history = load_chat_history(db, &req).await?;
    let mut messages = format_messages(&history, req.message_history.as_deref());
    messages.push_str(&format!("\n\n[USER]: <QUERY>{}</QUERY>", req.query));

    let (context_entries, trace) = initial_context(db, &req).await?;
    let entries = format_entries(&context_entries);
    let custom_instructions = load_custom_instructions();
    let personality = classify_personality(&req.query).await;
    let personality_prompt = personality
        .as_ref()
        .map(|p| p.prompt.as_str())
        .unwrap_or_default();

    let response = if is_openrouter_request(&req) {
        let registry = openrouter_client_registry(&req)?;
        B.DirectChat
            .with_client_registry(&registry)
            .call(
                messages.as_str(),
                entries.as_str(),
                custom_instructions.as_str(),
                personality_prompt,
            )
            .await
    } else if let Some(client) = selected_client(&req) {
        B.DirectChat
            .with_client(client)
            .call(
                messages.as_str(),
                entries.as_str(),
                custom_instructions.as_str(),
                personality_prompt,
            )
            .await
    } else {
        B.DirectChat
            .call(
                messages.as_str(),
                entries.as_str(),
                custom_instructions.as_str(),
                personality_prompt,
            )
            .await
    }
    .map_err(|err| err.to_string())?;

    Ok(ChatResponse {
        response,
        docs: docs_from_context(&context_entries),
        thread_id: req.thread_id.clone(),
        message_metadata: Some(response_metadata(
            &req,
            personality.as_ref(),
            context_entries,
            vec![],
            trace,
        )),
    })
}

fn trace_to_string(trace: &[SearchIteration]) -> String {
    trace
        .iter()
        .map(|step| {
            format!(
                "Iteration {}: tool={} query={:?} reasoning={} results={} new_entries={}",
                step.iteration,
                step.tool,
                step.query,
                step.reasoning,
                step.results_count,
                step.new_entries_added
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_name(call: &SearchToolCall) -> String {
    call.tool.to_string()
}

async fn execute_agent_tool(
    db: &Db,
    req: &ChatRequest,
    call: &SearchToolCall,
) -> Result<Vec<ContextItem>, String> {
    let limit = call
        .limit
        .unwrap_or(req.top_k.unwrap_or(DEFAULT_LIMIT as i32) as i64)
        .max(1) as usize;

    match call.tool {
        SearchToolType::VECTOR_SEARCH => {
            let query = call.query.as_deref().unwrap_or(req.query.as_str());
            let embedding = ai::embed_text(query).await?;
            let results = db
                .get_similar_entries(embedding, limit)
                .await
                .map_err(|err| err.to_string())?;
            Ok(results
                .into_iter()
                .map(|(entry, distance)| ContextItem::Entry {
                    entry,
                    distance: Some(distance),
                    source: "vector_search".to_string(),
                })
                .collect())
        }
        SearchToolType::RECENT_ENTRIES => {
            let entries = db
                .get_recent_entries(limit)
                .await
                .map_err(|err| err.to_string())?;
            Ok(entries
                .into_iter()
                .map(|entry| ContextItem::Entry {
                    entry,
                    distance: None,
                    source: "recent_entries".to_string(),
                })
                .collect())
        }
        SearchToolType::DATE_RANGE_SEARCH => {
            let start = call
                .start_date
                .clone()
                .ok_or_else(|| "DATE_RANGE_SEARCH missing start_date".to_string())?;
            let end = call
                .end_date
                .clone()
                .ok_or_else(|| "DATE_RANGE_SEARCH missing end_date".to_string())?;
            let entries = db
                .get_entries_by_date_range(start, end, Some(limit))
                .await
                .map_err(|err| err.to_string())?;
            Ok(entries
                .into_iter()
                .map(|entry| ContextItem::Entry {
                    entry,
                    distance: None,
                    source: "date_range_search".to_string(),
                })
                .collect())
        }
        SearchToolType::RECENT_LOGS => {
            let events = db
                .get_recent_log_events(limit)
                .await
                .map_err(|err| err.to_string())?;
            Ok(events
                .into_iter()
                .map(|event| ContextItem::LogEvent {
                    event,
                    source: "recent_logs".to_string(),
                })
                .collect())
        }
        SearchToolType::LOG_DATE_RANGE_SEARCH => {
            let start = call
                .start_date
                .clone()
                .ok_or_else(|| "LOG_DATE_RANGE_SEARCH missing start_date".to_string())?;
            let end = call
                .end_date
                .clone()
                .ok_or_else(|| "LOG_DATE_RANGE_SEARCH missing end_date".to_string())?;
            let events = db
                .get_log_events_by_date_range(start, end, Some(limit), None)
                .await
                .map_err(|err| err.to_string())?;
            Ok(events
                .into_iter()
                .map(|event| ContextItem::LogEvent {
                    event,
                    source: "log_date_range_search".to_string(),
                })
                .collect())
        }
        SearchToolType::LOG_TAG_SEARCH => {
            let tag = call
                .tag
                .clone()
                .ok_or_else(|| "LOG_TAG_SEARCH missing tag".to_string())?;
            let events = db
                .get_log_events_by_tags(vec![tag], Some(limit))
                .await
                .map_err(|err| err.to_string())?;
            Ok(events
                .into_iter()
                .map(|event| ContextItem::LogEvent {
                    event,
                    source: "log_tag_search".to_string(),
                })
                .collect())
        }
        SearchToolType::LOG_TAG_DRILLDOWN => {
            eprintln!(
                "[llm] LOG_TAG_DRILLDOWN requested but is not implemented yet; returning no context"
            );
            Ok(vec![])
        }
        SearchToolType::DONE => Ok(vec![]),
    }
}

pub async fn agent_chat<F>(mut emit: F, db: &Db, req: ChatRequest) -> Result<ChatResponse, String>
where
    F: FnMut(SearchIteration) -> Result<(), String>,
{
    let mut state = AgentSearchState::default();

    let recent_entries = db
        .get_recent_entries(RECENT_PRESEED_COUNT)
        .await
        .map_err(|err| err.to_string())?;
    let new_count = state.add_items(recent_entries.into_iter().map(|entry| ContextItem::Entry {
        entry,
        distance: None,
        source: "recent_entries_preseed".to_string(),
    }));
    let step = state.record_iteration(
        0,
        "RECENT_ENTRIES_PRESEED",
        "Always include recent entries for temporal context.",
        None,
        new_count as i32,
        new_count as i32,
    );
    emit(step)?;

    for iteration in 1..=MAX_AGENT_ITERATIONS {
        let call = B
            .AgentToolSelector
            .call(
                req.query.as_str(),
                state.context_string().as_str(),
                state.trace_string().as_str(),
                iteration,
                MAX_AGENT_ITERATIONS,
            )
            .await
            .map_err(|err| err.to_string())?;

        if matches!(call.tool, SearchToolType::DONE) {
            let step = state.record_iteration(
                iteration as i32,
                tool_name(&call),
                call.reasoning,
                None,
                0,
                0,
            );
            emit(step)?;
            break;
        }

        let query_for_trace = call.query.clone().or_else(|| {
            call.tag.clone().or_else(|| match (&call.start_date, &call.end_date) {
                (Some(start), Some(end)) => Some(format!("{start}..{end}")),
                _ => None,
            })
        });

        let results = match execute_agent_tool(db, &req, &call).await {
            Ok(results) => results,
            Err(err) => {
                eprintln!("[llm] agent tool {} failed: {err}", tool_name(&call));
                vec![]
            }
        };
        let results_count = results.len();
        let new_count = state.add_items(results);
        let step = state.record_iteration(
            iteration as i32,
            tool_name(&call),
            call.reasoning,
            query_for_trace,
            results_count as i32,
            new_count as i32,
        );
        emit(step)?;
    }

    let history = load_chat_history(db, &req).await?;
    let mut messages = format_messages(&history, req.message_history.as_deref());
    messages.push_str(&format!("\n\n[USER]: <QUERY>{}</QUERY>", req.query));

    let accumulated = state.context_string();
    let search_trace = state.trace_string();
    let custom_instructions = load_custom_instructions();
    let personality = classify_personality(&req.query).await;
    let personality_prompt = personality
        .as_ref()
        .map(|p| p.prompt.as_str())
        .unwrap_or_default();

    let response = if is_openrouter_request(&req) {
        let registry = openrouter_client_registry(&req)?;
        B.AgentSynthesizer
            .with_client_registry(&registry)
            .call(
                req.query.as_str(),
                messages.as_str(),
                accumulated.as_str(),
                search_trace.as_str(),
                custom_instructions.as_str(),
                personality_prompt,
            )
            .await
    } else if let Some(client) = selected_client(&req) {
        B.AgentSynthesizer
            .with_client(client)
            .call(
                req.query.as_str(),
                messages.as_str(),
                accumulated.as_str(),
                search_trace.as_str(),
                custom_instructions.as_str(),
                personality_prompt,
            )
            .await
    } else {
        B.AgentSynthesizer
            .call(
                req.query.as_str(),
                messages.as_str(),
                accumulated.as_str(),
                search_trace.as_str(),
                custom_instructions.as_str(),
                personality_prompt,
            )
            .await
    }
    .map_err(|err| err.to_string())?;

    let context_entries = state.context_entries();
    let context_logs = state.context_logs();
    Ok(ChatResponse {
        response,
        docs: docs_from_context(&context_entries),
        thread_id: req.thread_id.clone(),
        message_metadata: Some(response_metadata(
            &req,
            personality.as_ref(),
            context_entries,
            context_logs,
            state.search_trace,
        )),
    })
}
