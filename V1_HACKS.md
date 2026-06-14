# V1 Hacks / Deferred Cleanup

A running list of pragmatic shortcuts in the current MVP implementation.

## Home / Recent Panels

- **Recent Threads are limited client-side.**
  - `apiService.getThreads()` fetches all threads, then Home sorts/slices by measured panel limit.
  - Eventually add backend-supported `limit` / ordering.

- **Recent list item-height limits are estimated.**
  - `useMeasuredRecentList` computes limits from approximate item heights.
  - Expanded entries can make the real item count/scroll height diverge from the estimate.

- **Recent entries expansion state is local only.**
  - Expanded/collapsed entries reset on navigation/remount.
  - Fine for now; persist only if users care.

## Tags / Taxonomy

- **Tag taxonomy is computed ad hoc.**
  - `list_tags` scans entries/logs/threads and aggregates counts on request.
  - Eventually replace with a central taxonomy table/index updated during ingest and writes.

- **Typeahead is client-filtered from a loaded tag list.**
  - Good enough for small taxonomies.
  - May need server-side search/ranking if tags get large.

- **Tag grammar is duplicated frontend/backend.**
  - Both currently support `#[letters numbers _ - /]+`.
  - Should eventually centralize this contract or add shared test fixtures.

- **Hierarchical tags are only strings.**
  - `#Work/EK` works, but no parent/child model exists yet.
  - Future taxonomy can derive parent nodes like `#Work` from `#Work/EK`.

## Composer

- **Date/time picker is custom and minimal.**
  - Hour/minute fields manually validate/clamp.
  - No rich date navigation, relative dates, or keyboard polish yet.

- **Composer chat submission does not parse tags.**
  - Intentional for v1: chat receives raw text exactly as typed.

- **Log submission strips tags from text with regex.**
  - Works for common cases, but a real tokenizer would be more robust.

## Chat

- **Initial chat auto-send is effect-driven.**
  - Guarded by ephemeral `launchId` to avoid duplicate sends.
  - Still a UI-side orchestration pattern; could become a clearer command/event flow later.

- **Thread loading lives in `ChatInterface`.**
  - Better than the old `window.loadThreadIntoChat` hack, but chat/page state boundaries could be cleaner.

## Styling / Layout

- **Styling is plain CSS with some shared class coupling.**
  - `RecentListPanel` uses `home-panel` / `home-list` classes from Home styles.
  - Fine for MVP, but component CSS boundaries are a bit porous.

- **Tauri titlebar customization was reverted.**
  - Default native titlebar remains because overlay dragging was unreliable.
  - Revisit later with a proper Tauri drag-region/titlebar implementation.

## Data / API Shape

- **No dedicated Recent Chats API.**
  - Threads are fetched wholesale.
  - Add `get_recent_threads(limit)` if thread volume grows.

- **No dedicated tag source metadata.**
  - `TagSummary` only has `{ tag, count }`.
  - Later we may want source counts: entries/logs/threads, last seen, created at, etc.
