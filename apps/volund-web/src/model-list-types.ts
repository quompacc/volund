import type { ModelSummary, Page } from "./types";

/** The filter of a saved model list view (ADR 0032). */
export interface SavedViewQuery {
  q?: string;
  tag?: string;
  kind?: string;
  sort?: string;
}

export interface SavedView {
  id: string;
  name: string;
  query: SavedViewQuery;
  createdAtUnixMs: number;
}

/** One page of `GET /api/v1/models/search`. */
export interface ModelSearchPage extends Page<ModelSummary> {
  fileLinkTotal: number;
}
