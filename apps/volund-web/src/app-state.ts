export type AppView = "dashboard" | "models" | "model" | "model-problems" | "collections" | "tags" | "authors" | "raw" | "imports" | "import-history" | "administration";

const APP_VIEWS: AppView[] = ["dashboard", "models", "model", "model-problems", "collections", "tags", "authors", "raw", "imports", "import-history", "administration"];

export function readAppView(search: string): AppView {
  const parameters = new URLSearchParams(search);
  const requested = parameters.get("view") as AppView | null;
  if (requested && APP_VIEWS.includes(requested)) return requested;
  return parameters.has("root") ? "raw" : "dashboard";
}

export function readModelId(search: string): string | null {
  return new URLSearchParams(search).get("model");
}

export function appViewUrl(view: AppView, modelId?: string): string {
  if (view === "raw") return "?view=raw&root=cad";
  const parameters = new URLSearchParams({ view });
  if ((view === "model" || view === "model-problems") && modelId) parameters.set("model", modelId);
  return `?${parameters}`;
}
