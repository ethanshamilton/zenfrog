import { Channel, invoke } from '@tauri-apps/api/core'
import type { Thread, ThreadMessage, SearchIteration, MessageMetadata, Document as JournalDocument } from '../types'

export interface Entry {
  entry_id: string
  date: string
  title: string
  text: string
  tags: string[]
  embedding?: number[] | null
  entry_type: string
}

export interface RetrievedDoc {
  entry: Entry
  distance: number | null
}

export interface CreateLogEventInput {
  datetime: string
  text: string
  tags: string[]
}

export interface LogEvent extends CreateLogEventInput {
  log_event_id: string
}

export interface TaxonomyTag {
  tag: string
  description: string
  color: string | null
  broader: string[]
  narrower: string[]
  count: number
}

export interface UpdateTaxonomyTagInput {
  tag: string
  description: string
  color: string | null
}

export interface RenameTaxonomyTagInput {
  old_tag: string
  new_tag: string
}

export interface TagInstance {
  tag: string
  source_type: 'journal' | 'log_event' | 'thread' | string
  source_id: string
  title: string | null
  text: string | null
  datetime: string | null
}

export type LogEventOrder = 'ascending' | 'descending'

export interface ChatRequest {
  query: string
  top_k?: number
  provider: string
  model: string
  thread_id?: string
  message_history?: Array<{
    sender: 'user' | 'assistant'
    text: string
    timestamp: Date
  }>
  existing_docs?: JournalDocument[]
}

export interface ChatResponse {
  response: string
  docs: RetrievedDoc[]
  thread_id?: string
  message_metadata?: MessageMetadata | null
}

export interface StatusResponse {
  status: string
}

type StreamEvent =
  | { type: 'SearchIteration'; data: SearchIteration }
  | { type: 'ChatResponse'; data: ChatResponse }
  | { type: 'Error'; data: { error: string } }

export const apiService = {
  // Thread management methods
  async createThread(title?: string, initialMessage?: string): Promise<{ thread_id: string; created_at: string }> {
    return invoke('create_thread', {
      req: {
        title,
        initial_message: initialMessage,
      },
    })
  },

  async getThreads(): Promise<Thread[]> {
    return invoke('get_threads')
  },

  async getThread(threadId: string): Promise<Thread> {
    return invoke('get_thread', { threadId })
  },

  async getThreadMessages(threadId: string): Promise<ThreadMessage[]> {
    return invoke('get_thread_messages', { threadId })
  },

  async deleteThread(threadId: string): Promise<void> {
    return invoke('delete_thread', { threadId })
  },

  async updateThreadTitle(threadId: string, title: string): Promise<void> {
    return invoke('update_thread_title', {
      threadId,
      req: { title },
    })
  },

  async generateThreadTitle(threadId: string): Promise<{ title: string }> {
    return invoke('generate_thread_title', { threadId })
  },

  async addMessageToThread(
    threadId: string,
    role: string,
    content: string,
    metadata?: MessageMetadata | null,
  ): Promise<ThreadMessage> {
    return invoke('add_message_to_thread', {
      threadId,
      req: {
        role,
        content,
        metadata,
      },
    })
  },

  async getRecentEntries(n?: number): Promise<Entry[]> {
    return invoke('get_recent_entries', { n })
  },

  async listTaxonomyTags(): Promise<TaxonomyTag[]> {
    return invoke('list_taxonomy_tags')
  },

  async getTaxonomyTag(tag: string): Promise<TaxonomyTag> {
    return invoke('get_taxonomy_tag', { tag })
  },

  async updateTaxonomyTag(req: UpdateTaxonomyTagInput): Promise<TaxonomyTag> {
    return invoke('update_taxonomy_tag', { req })
  },

  async renameTaxonomyTag(req: RenameTaxonomyTagInput): Promise<TaxonomyTag> {
    return invoke('rename_taxonomy_tag', { req })
  },

  async listTagInstances(tag: string, limit?: number): Promise<TagInstance[]> {
    return invoke('list_tag_instances', { tag, limit })
  },

  async resolveTagColors(tags: string[]): Promise<Record<string, string | null>> {
    return invoke('resolve_tag_colors', { tags })
  },

  async deleteTag(tag: string): Promise<void> {
    return invoke('delete_tag', { tag })
  },

  async createLogEvent(event: CreateLogEventInput): Promise<LogEvent> {
    return invoke('create_log_event', { event })
  },

  async deleteLogEvent(logEventId: string): Promise<void> {
    return invoke('delete_log_event', { logEventId })
  },

  async listLogEvents(args: {
    order?: LogEventOrder
    limit?: number
    tags?: string[]
  } = {}): Promise<LogEvent[]> {
    return invoke('list_log_events', args)
  },

  async queryJournalStream(
    request: ChatRequest,
    onIteration: (iteration: SearchIteration) => void,
    onComplete: (response: ChatResponse) => void,
  ): Promise<void> {
    let streamError: Error | null = null

    const channel = new Channel<StreamEvent>()
    channel.onmessage = (event) => {
      switch (event.type) {
        case 'SearchIteration':
          onIteration(event.data)
          break
        case 'ChatResponse':
          onComplete(event.data)
          break
        case 'Error':
          streamError = new Error(event.data.error || 'Stream error occurred')
          break
      }
    }

    await invoke('journal_chat_agent_stream', { req: request, onEvent: channel })

    if (streamError) {
      throw streamError
    }
  },

  async checkStatus(): Promise<StatusResponse> {
    return invoke('get_status')
  },
}
