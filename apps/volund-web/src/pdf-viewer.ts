import type { PDFDocumentLoadingTask, PDFDocumentProxy, PDFPageProxy, RenderTask } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

export function pdfViewerMarkup(name: string, byteSize: string, sourceUrl: string): string {
  const safeName = escapePdfMarkup(name);
  const safeSize = escapePdfMarkup(byteSize);
  const safeUrl = escapePdfMarkup(sourceUrl);
  return `<div class="pdf-viewer"><div class="pdf-toolbar">
    <button type="button" data-pdf-previous aria-label="Vorherige Seite">←</button>
    <label>Seite <input data-pdf-page type="number" min="1" value="1" aria-label="PDF-Seite"></label>
    <span data-pdf-pages>/ —</span>
    <button type="button" data-pdf-next aria-label="Nächste Seite">→</button>
    <i></i><button type="button" data-pdf-zoom-out aria-label="Verkleinern">−</button>
    <span data-pdf-zoom>100 %</span><button type="button" data-pdf-zoom-in aria-label="Vergrößern">＋</button>
    <a href="${safeUrl}" target="_blank" rel="noopener">Original öffnen ↗</a>
  </div><div class="pdf-canvas-wrap"><canvas data-pdf-canvas aria-label="${safeName}"></canvas>
    <div class="pdf-state" data-pdf-state>PDF wird geladen …</div></div>
  <div class="pdf-caption"><strong>${safeName}</strong><span>PDF · ${safeSize}</span></div></div>`;
}

function escapePdfMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

export function normalizedPdfPage(requested: number, total: number): number {
  return Math.min(Math.max(Math.trunc(requested) || 1, 1), Math.max(total, 1));
}

export function steppedPdfScale(current: number, direction: -1 | 1): number {
  return Math.min(Math.max(Math.round((current + direction * 0.2) * 10) / 10, 0.4), 3);
}

export class PdfViewer {
  private readonly canvas: HTMLCanvasElement;
  private readonly pageInput: HTMLInputElement;
  private readonly pages: HTMLElement;
  private readonly zoom: HTMLElement;
  private readonly state: HTMLElement;
  private document?: PDFDocumentProxy;
  private loadingTask?: PDFDocumentLoadingTask;
  private renderTask?: RenderTask;
  private pageNumber = 1;
  private scale = 1;
  private renderGeneration = 0;

  constructor(private readonly host: HTMLElement, private readonly sourceUrl: string) {
    this.canvas = host.querySelector<HTMLCanvasElement>("[data-pdf-canvas]")!;
    this.pageInput = host.querySelector<HTMLInputElement>("[data-pdf-page]")!;
    this.pages = host.querySelector<HTMLElement>("[data-pdf-pages]")!;
    this.zoom = host.querySelector<HTMLElement>("[data-pdf-zoom]")!;
    this.state = host.querySelector<HTMLElement>("[data-pdf-state]")!;
    host.querySelector("[data-pdf-previous]")!.addEventListener("click", () => void this.changePage(this.pageNumber - 1));
    host.querySelector("[data-pdf-next]")!.addEventListener("click", () => void this.changePage(this.pageNumber + 1));
    host.querySelector("[data-pdf-zoom-out]")!.addEventListener("click", () => void this.changeScale(-1));
    host.querySelector("[data-pdf-zoom-in]")!.addEventListener("click", () => void this.changeScale(1));
    this.pageInput.addEventListener("change", () => void this.changePage(Number(this.pageInput.value)));
  }

  async load(): Promise<void> {
    const pdfjs = await import("pdfjs-dist");
    pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
    this.loadingTask = pdfjs.getDocument({ url: this.sourceUrl });
    this.document = await this.loadingTask.promise;
    this.pageInput.max = String(this.document.numPages);
    this.pages.textContent = `/ ${this.document.numPages}`;
    await this.render();
  }

  dispose(): void {
    this.renderGeneration += 1;
    this.renderTask?.cancel();
    this.renderTask = undefined;
    void this.loadingTask?.destroy();
    this.loadingTask = undefined;
    this.document = undefined;
  }

  private async changePage(requested: number): Promise<void> {
    if (!this.document) return;
    this.pageNumber = normalizedPdfPage(requested, this.document.numPages);
    await this.render();
  }

  private async changeScale(direction: -1 | 1): Promise<void> {
    this.scale = steppedPdfScale(this.scale, direction);
    await this.render();
  }

  private async render(): Promise<void> {
    if (!this.document) return;
    const generation = ++this.renderGeneration;
    this.renderTask?.cancel();
    this.state.hidden = false;
    const page: PDFPageProxy = await this.document.getPage(this.pageNumber);
    if (generation !== this.renderGeneration) return;
    const viewport = page.getViewport({ scale: this.scale });
    const pixelRatio = Math.min(window.devicePixelRatio, 2);
    this.canvas.width = Math.floor(viewport.width * pixelRatio);
    this.canvas.height = Math.floor(viewport.height * pixelRatio);
    this.canvas.style.width = `${Math.floor(viewport.width)}px`;
    this.canvas.style.height = `${Math.floor(viewport.height)}px`;
    const context = this.canvas.getContext("2d");
    if (!context) throw new Error("PDF-Canvas ist nicht verfügbar.");
    const renderTask = page.render({
      canvas: this.canvas,
      canvasContext: context,
      viewport,
      transform: pixelRatio === 1 ? undefined : [pixelRatio, 0, 0, pixelRatio, 0, 0],
    });
    this.renderTask = renderTask;
    try {
      await renderTask.promise;
    } catch (error) {
      if (generation !== this.renderGeneration) return;
      throw error;
    } finally {
      if (this.renderTask === renderTask) this.renderTask = undefined;
    }
    if (generation !== this.renderGeneration) return;
    this.pageInput.value = String(this.pageNumber);
    this.zoom.textContent = `${Math.round(this.scale * 100)} %`;
    this.state.hidden = true;
  }
}
