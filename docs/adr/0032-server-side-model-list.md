# ADR 0032: Server-side model list and saved views

- Status: Accepted
- Date: 2026-10-26

## Context

The model list loaded `GET /api/v1/models`, the complete catalog with every
aggregate, on each visit to the dashboard, the model list, and the navigation
counter. Tag filtering happened in the browser. That works for dozens of models
but not for the thousands a long-lived workshop archive accumulates. The list
also had no text search, no sorting, and no way to share or keep a filtered view.

## Decision

- `GET /api/v1/models/search` returns one page plus totals:
  `{items, limit, offset, total, fileLinkTotal}`.
  - Parameters: `q`, text matched case-insensitively and literally against
    name, slug, description, author, and tag names; `tag` (tag ID); `kind`;
    `sort` (`updated` by default, `name`, `created`, each with a stable
    tie-break); `limit` (1–200, default 48); and `offset`.
  - Invalid values are rejected with 400, not silently ignored.
  - `fileLinkTotal` sums linked files over the whole result set, so the
    dashboard needs no full download.
- `GET /api/v1/models` stays unchanged for the pickers that need every model
  (import extension, collection membership). Replacing those pickers with
  search-as-you-type is a separate step.
- The browser model list keeps its whole filter state in the page URL (`q`,
  `tag`, `kind`, `sort`, `page`), so a view can be bookmarked, shared, or
  restored with the back button.
  - Typing is debounced, and a response that arrives after a newer request is
    discarded.
  - A page beyond the end is clamped to the last page.
- The dashboard asks for the six most recently edited models and reads the
  totals. The navigation counter asks for one item and reads `total`.
- **Saved views** (migration 0034, `/api/v1/saved-views`):
  - Each account can keep up to 50 named filter sets. Names are unique per
    account, ignoring case.
  - A view stores only `q`, `tag`, `kind`, and `sort`, never the page.
  - It is normalized through the same validation as a live search, so it can
    never hold a query the search would reject.
  - Views are private display state. They are neither shared nor used as an
    access rule.
  - Selecting a view in the toolbar applies it; the current filters can be
    saved under a new name or the selected view deleted.
- Offset pagination is sufficient at this scale; the stable sort keys keep
  pages deterministic. Keyset pagination can follow if offsets become slow.

## Consequences

- The model list cost no longer grows with the catalog size.
- Clients that relied on browser-side tag filtering of `/models` keep working;
  the new list does not depend on it.
