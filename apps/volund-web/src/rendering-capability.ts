/**
 * WebGL fallback (ADR 0036): detect whether 3D previews render on a GPU, only in
 * software, or not at all, and gate large previews on software rendering.
 */
export type RenderingCapability = "hardware" | "software" | "none";

/** Previews above this size are only loaded on request with software rendering. */
export const SOFTWARE_PREVIEW_LIMIT_BYTES = 8 * 1024 * 1024;
/** File tiles above this size get no live preview with software rendering. */
export const SOFTWARE_TILE_LIMIT_BYTES = 2 * 1024 * 1024;

const SOFTWARE_RENDERERS = /swiftshader|llvmpipe|softpipe|software|basic render/i;

type ContextFactory = (attributes?: WebGLContextAttributes) => WebGLRenderingContext | WebGL2RenderingContext | null;

/** Classify a browser's WebGL support from a context factory (testable without a GPU). */
export function classifyRendering(createContext: ContextFactory): RenderingCapability {
  let context: WebGLRenderingContext | WebGL2RenderingContext | null = null;
  try {
    context = createContext({ failIfMajorPerformanceCaveat: true });
    if (!context) return createContext() ? "software" : "none";
    const debug = context.getExtension("WEBGL_debug_renderer_info");
    const renderer = String(context.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : context.RENDERER) ?? "");
    return SOFTWARE_RENDERERS.test(renderer) ? "software" : "hardware";
  } catch {
    return "none";
  } finally {
    context?.getExtension("WEBGL_lose_context")?.loseContext();
  }
}

let cached: RenderingCapability | undefined;

/** The current browser's capability, probed once per page with a throwaway canvas. */
export function renderingCapability(doc: Document = document): RenderingCapability {
  if (cached) return cached;
  const canvas = doc.createElement("canvas");
  cached = classifyRendering((attributes) =>
    (canvas.getContext("webgl2", attributes) as WebGL2RenderingContext | null)
    ?? (doc.createElement("canvas").getContext("webgl", attributes) as WebGLRenderingContext | null));
  return cached;
}

/** Test seam: forget or force the probed capability. */
export function resetRenderingCapability(value?: RenderingCapability): void { cached = value; }

export type PreviewGate = "render" | "confirm" | "unavailable";

export function previewGate(capability: RenderingCapability, bytes: number, requested: boolean): PreviewGate {
  if (capability === "none") return "unavailable";
  if (capability === "software" && bytes > SOFTWARE_PREVIEW_LIMIT_BYTES && !requested) return "confirm";
  return "render";
}

export function tileAllowed(capability: RenderingCapability, bytes: number): boolean {
  return capability === "hardware" || (capability === "software" && bytes <= SOFTWARE_TILE_LIMIT_BYTES);
}

export function previewGateMarkup(gate: Exclude<PreviewGate, "render">, name: string, size: string): string {
  if (gate === "unavailable") {
    return `<div class="model-generic-preview preview-fallback"><span class="icon icon-alert" aria-hidden="true"></span><h2>${escapeMarkup(name)}</h2>
      <p>3D-Vorschau nicht verfügbar: Dieser Browser stellt kein WebGL bereit. Struktur, Dateiangaben und der Download des Originals bleiben nutzbar.</p></div>`;
  }
  return `<div class="model-generic-preview preview-fallback"><span class="icon icon-alert" aria-hidden="true"></span><h2>${escapeMarkup(name)}</h2>
    <p>Dieser Browser rendert 3D nur in Software. Die Vorschau (${escapeMarkup(size)}) kann ihn stark verlangsamen.</p>
    <button type="button" class="primary-action" data-load-preview-anyway>Vorschau trotzdem laden</button></div>`;
}

const requestedPreviews = new Set<string>();

/**
 * Render the gate into `stage` unless the preview may load. Returns true when
 * the caller should render; the button re-opens the file with the request noted.
 */
export function admitPreview(stage: HTMLElement, fileId: string, name: string, bytes: number, size: string, reopen: () => void): boolean {
  const gate = previewGate(renderingCapability(), bytes, requestedPreviews.has(fileId));
  if (gate === "render") return true;
  stage.innerHTML = previewGateMarkup(gate, name, size);
  stage.querySelector("[data-load-preview-anyway]")?.addEventListener("click", () => {
    requestedPreviews.add(fileId);
    reopen();
  });
  return false;
}

function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
