
// INTENT CLASSIFICATION

enum SearchOptions {
  VECTOR
  RECENT
}

function IntentClassifier(query: string) -> SearchOptions {
  client "openai/gpt-5-mini"
  prompt #"
    <INSTRUCTIONS>
    Determine which search method will be best for the following query:
    </INSTRUCTIONS>
    <QUERY>
    {{query}}
    </QUERY>
    <OUTPUT_FORMAT>
    {{ctx.output_format}}
    </OUTPUT_FORMAT>
  "#
}

// PERSONALITY CLASSIFICATION

function PersonalityClassifier(query: string, personality_options: string) -> string {
  client "openai/gpt-5-mini"
  prompt #"
    <INSTRUCTIONS>
    You are selecting a response personality for a journal chat assistant.
    Based on the user's query, choose the most appropriate personality from the options below.
    Return ONLY the exact title of the selected personality.

    If none of the personalities seem like a good fit, return "default".
    </INSTRUCTIONS>

    <PERSONALITY_OPTIONS>
    {{personality_options}}
    </PERSONALITY_OPTIONS>

    <QUERY>
    {{query}}
    </QUERY>

    <OUTPUT_FORMAT>
    {{ctx.output_format}}
    </OUTPUT_FORMAT>
  "#
}

// THREAD TITLE GENERATION

function GenerateThreadTitle(messages: string) -> string {
  client "openai/gpt-5-mini"
  prompt #"
    <INSTRUCTIONS>
    Generate a concise, descriptive title for the following chat transcript.
    Constraints:
    - 6 words or fewer
    - No surrounding quotes or trailing punctuation
    - Capture the main topic, not generic phrasing like "Chat about journal"
    Return ONLY the title text.
    </INSTRUCTIONS>

    <CHAT_HISTORY>
    {{messages}}
    </CHAT_HISTORY>

    <OUTPUT_FORMAT>
    {{ctx.output_format}}
    </OUTPUT_FORMAT>
  "#
}

// DIRECT CHAT

function DirectChat(messages: string, entries: string, custom_instructions: string, personality_prompt: string) -> string {
    client "openai/gpt-5"
    prompt #"
        <INSTRUCTIONS>
        {% if personality_prompt %}
        {{personality_prompt}}
        {% else %}
        Provide an empathetic and analytical response to the user.
        {% endif %}

        Additional custom instructions from CUSTOM_INSTRUCTIONS.md:
        {{custom_instructions}}
        </INSTRUCTIONS>
        <RELEVANT_ENTRIES>
        {{entries}}
        </RELEVANT_ENTRIES>
        <CHAT_HISTORY>
        {{messages}}
        </CHAT_HISTORY>
        <OUTPUT_FORMAT>
        {{ctx.output_format}}
        </OUTPUT_FORMAT>
    "#
}

// AGENTIC SEARCH LOOP

enum SearchToolType {
  VECTOR_SEARCH @description("Semantic search over long-form journal entries using embeddings. Best for topical, emotional, conceptual, or fuzzy queries.")
  RECENT_ENTRIES @description("Retrieve recent long-form journal entries. Best for reflective journal context about what has been happening lately.")
  DATE_RANGE_SEARCH @description("Retrieve long-form journal entries between explicit dates. Best for months, years, named periods, or user-specified date windows.")
  RECENT_LOGS @description("Retrieve recent lightweight log events. Best for quick timeline questions, recent activities, and terse logged facts.")
  LOG_DATE_RANGE_SEARCH @description("Retrieve lightweight log events between explicit dates or datetimes.")
  LOG_TAG_SEARCH @description("Retrieve lightweight log events matching a specific exact tag.")
  LOG_TAG_DRILLDOWN @description("Stub for hierarchical log tag exploration. Use when tag-based log retrieval seems appropriate but the exact tag is unclear.")
  DONE @description("Stop searching because enough relevant context has been gathered or no available search would help.")
}

class SearchToolCall {
  tool SearchToolType @description("The retrieval tool to call next")
  reasoning string @description("Explain why this tool is needed and what you hope to find")
  query string? @description("Search query for VECTOR_SEARCH")
  start_date string? @description("Start date for DATE_RANGE_SEARCH or LOG_DATE_RANGE_SEARCH. Use YYYY-MM-DD or RFC3339 when available.")
  end_date string? @description("End date for DATE_RANGE_SEARCH or LOG_DATE_RANGE_SEARCH. Use YYYY-MM-DD or RFC3339 when available.")
  tag string? @description("Exact tag for LOG_TAG_SEARCH, or candidate/parent tag for LOG_TAG_DRILLDOWN")
  limit int? @description("Optional limit on number of results")
}

function AgentToolSelector(
  user_query: string,
  accumulated_context: string,
  search_trace: string,
  iteration: int,
  max_iterations: int
) -> SearchToolCall {
  client "openai/gpt-5-mini"
  prompt #"
    <INSTRUCTIONS>
    You are an intelligent search agent for a personal journal database. Your task is to select the best search tool to gather relevant journal entries for answering the user's query.

    Available tools:
    - VECTOR_SEARCH: Semantic search using embeddings over long-form journal entries. Best for finding entries about specific topics, emotions, or concepts. Requires a search query.
    - RECENT_ENTRIES: Get the most recent long-form journal entries. Best for reflective questions about "lately", "recently", or current state.
    - DATE_RANGE_SEARCH: Retrieve long-form journal entries between dates. Best for explicit time windows, comparisons across periods, months, years, or phrases like "last summer". Requires start_date and end_date.
    - RECENT_LOGS: Get the most recent lightweight log events. Best for quick timeline questions, recent activities, or terse logged facts.
    - LOG_DATE_RANGE_SEARCH: Retrieve lightweight log events between dates/datetimes. Best when the user asks what was logged in a specific period. Requires start_date and end_date.
    - LOG_TAG_SEARCH: Retrieve lightweight log events with an exact tag. Requires tag.
    - LOG_TAG_DRILLDOWN: Currently a stub for hierarchical tag exploration. Use sparingly when exact tag-based log retrieval seems important but the exact tag is unclear. It does not retrieve logs by itself.
    - DONE: Select this when you have gathered enough context to answer the user's query, or if no more searches would be helpful.

    Source distinctions:
    - Journal entries are long-form reflective writing.
    - Log events are short timestamped records. Prefer log tools when the user says "logs", "logged", "events", "quick notes", "timeline", or asks what they did at/around a time.
    - For broad timeline/current-state questions, using both recent journal entries and recent logs can be helpful.

    Consider:
    - What information is still missing to fully answer the query?
    - Have we already gathered enough context?
    - Would a different search strategy find new relevant entries?
    - IMPORTANT: Check the search trace to see what searches have already been performed. Do not repeat searches.
    </INSTRUCTIONS>

    <USER_QUERY>
    {{user_query}}
    </USER_QUERY>

    <SEARCH_TRACE>
    {{search_trace}}
    </SEARCH_TRACE>

    <ACCUMULATED_CONTEXT>
    {{accumulated_context}}
    </ACCUMULATED_CONTEXT>

    <ITERATION_INFO>
    Current iteration: {{iteration}} of {{max_iterations}}
    </ITERATION_INFO>

    <OUTPUT_FORMAT>
    {{ctx.output_format}}
    </OUTPUT_FORMAT>
  "#
}

// Note: AgentSynthesizer uses default client but is overridden via ClientRegistry at runtime
// based on the user's provider/model selection in the UI
function AgentSynthesizer(
  user_query: string,
  chat_history: string,
  accumulated_context: string,
  search_trace: string,
  custom_instructions: string,
  personality_prompt: string
) -> string {
  client "openai/gpt-5"
  prompt #"
    <INSTRUCTIONS>
    {% if personality_prompt %}
    {{personality_prompt}}

    Use the journal entries retrieved through multiple searches to inform your response.
    {% else %}
    Provide an empathetic and analytical response to the user based on the journal entries retrieved through multiple searches.
    {% endif %}

    The search trace shows how the agent gathered information to answer this query. Use all relevant context to provide a comprehensive, thoughtful response.

    Be specific and reference actual content from the entries when relevant. If the entries don't contain enough information to fully answer the query, acknowledge this honestly.

    Additional custom instructions from CUSTOM_INSTRUCTIONS.md:
    {{custom_instructions}}
    </INSTRUCTIONS>

    <SEARCH_TRACE>
    {{search_trace}}
    </SEARCH_TRACE>

    <ACCUMULATED_JOURNAL_ENTRIES>
    {{accumulated_context}}
    </ACCUMULATED_JOURNAL_ENTRIES>

    <CHAT_HISTORY>
    {{chat_history}}
    </CHAT_HISTORY>

    <USER_QUERY>
    {{user_query}}
    </USER_QUERY>

    <OUTPUT_FORMAT>
    {{ctx.output_format}}
    </OUTPUT_FORMAT>
  "#
}
