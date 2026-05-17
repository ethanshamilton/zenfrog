export interface Document {
    id: number
    title: string
    content: string
}

export interface Thread {
    thread_id: string
    title: string
    tags?: string[]
    created_at: string
    updated_at: string
}

export interface SearchIteration {
    iteration: number
    tool: string
    reasoning: string
    query: string | null
    results_count: number
    new_entries_added: number
}

export interface MessageModelMetadata {
    provider: string
    model: string
}

export interface MessagePersonalityMetadata {
    title: string | null
    description: string | null
    prompt: string | null
}

export interface MessageContextEntry {
    date: string | null
    title: string
    entry_type: string
    text: string
    tags: string[]
    distance: number | null
    source: string
}

export interface MessageContextChat {
    thread_id: string
    message_id?: string | null
    role?: string | null
    content: string
    timestamp?: string | null
}

export interface MessageMetadata {
    model: MessageModelMetadata
    personality?: MessagePersonalityMetadata | null
    context_entries: MessageContextEntry[]
    context_chats: MessageContextChat[]
    retrieval_trace: SearchIteration[]
}

export interface ThreadMessage {
    message_id: string
    thread_id: string
    timestamp: string
    role: 'user' | 'assistant'
    content: string
    metadata?: MessageMetadata | null
}
