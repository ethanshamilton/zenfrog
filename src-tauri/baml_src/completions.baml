
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
  VECTOR_SEARCH
  RECENT_ENTRIES
  DATE_RANGE_SEARCH
  DONE
}

class SearchToolCall {
  tool SearchToolType
  reasoning string @description("Explain why this tool is needed and what you hope to find")
  query string? @description("Search query for VECTOR_SEARCH")
  start_date string? @description("Start date for DATE_RANGE_SEARCH (YYYY-MM-DD)")
  end_date string? @description("End date for DATE_RANGE_SEARCH (YYYY-MM-DD)")
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
    - VECTOR_SEARCH: Semantic search using embeddings. Best for finding entries about specific topics, emotions, or concepts. Requires a search query.
    - RECENT_ENTRIES: Get the most recent journal entries. Best for questions about "lately", "recently", or current state.
    - DATE_RANGE_SEARCH: Retrieve entries between dates. Best for explicit time windows, comparisons across periods, months, years, or phrases like "last summer". Requires start_date and end_date in YYYY-MM-DD.
    - DONE: Select this when you have gathered enough context to answer the user's query, or if no more searches would be helpful.

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
