import type { AppView } from "./app-state";
import { catalogApi, identityApi, requestJson } from "./api";
import { assemblyTreeMarkup, mountAssemblyTree, parseAssemblyManifest } from "./assembly-tree";
import { mountProblemCenter } from "./problem-center";
import { formatBytes } from "./format";
import { hideFileContextMenu } from "./file-context-menu";
import { mountImportHistory, mountImports, queueModelExtension } from "./imports";
import { mountModelEditor } from "./model-editor";
import { mountModelHistory } from "./model-history";
import { bindCatalogHistories } from "./catalog-history";
import { mountAuthors } from "./authors";
import { mountCollections, mountTags } from "./metadata-catalog";
import { fileDetailMarkup, fileGlyph, modelCardMarkup, modelFileFilterOptions, modelFilesMarkup, modelProblemState, slicerHandoffStatusMarkup, thumbnailPanelMarkup } from "./model-presentation";
import { PdfViewer, pdfViewerMarkup } from "./pdf-viewer";
import { preferredModelFile, settingString } from "./preferences";
import type { Artifact, ModelFile, ModelSummary } from "./types";
import { CadViewer, restoreViewerStage } from "./viewer";

type Navigate = (view: AppView, modelId?: string) => void;

export function mountMockView(
  host: HTMLElement,
  view: Exclude<AppView, "raw" | "administration">,
  navigate: Navigate,
  modelId: string | null,
  canEdit: boolean,
  canManageMetadata: boolean,
): () => void {
  const cleanup: Array<() => void> = [];
  const viewHost = document.createElement("div");
  viewHost.className = "mock-view-host";
  host.replaceChildren(viewHost);
  cleanup.push(() => viewHost.remove());
  if (view === "dashboard" || view === "models") mountModelCatalog(viewHost, view);
  else if (view === "model") mountModel(viewHost, modelId, cleanup, canEdit, canManageMetadata, navigate);
  else if (view === "model-problems") mountModelProblems(viewHost, modelId, canEdit, navigate);
  else if (view === "imports") mountImports(viewHost, (id) => navigate("model", id));
  else if (view === "import-history") mountImportHistory(viewHost, (id) => navigate("model", id), () => navigate("imports"));
  else if (view === "authors") mountAuthors(viewHost, canManageMetadata);
  else if (view === "collections") mountCollections(viewHost, canEdit, canManageMetadata);
  else mountTags(viewHost, canManageMetadata);
  const handleNavigation = (event: Event): void => {
    const target = (event.target as HTMLElement).closest<HTMLElement>("[data-open-model], [data-navigate]");
    if (target?.dataset.openModel) navigate("model", target.dataset.openModel);
    else if (target?.dataset.navigate) navigate(target.dataset.navigate as AppView, target.dataset.modelId);
  };
  viewHost.addEventListener("click", handleNavigation);
  cleanup.push(() => viewHost.removeEventListener("click", handleNavigation));
  return () => cleanup.forEach((dispose) => dispose());
}

function mountModelCatalog(host: HTMLElement, view: "dashboard" | "models"): void {
  const dashboard = view === "dashboard";
  host.innerHTML = `<div class="page-scroll"><header class="page-hero"><div><p class="eyebrow">${dashboard ? "DIE SCHMIEDE" : "KATALOG"}</p>
    <h1>${dashboard ? "Willkommen in VÖLUND" : "Alle Modelle"}</h1><p>Deine echten Modelle, Fertigungsdaten und Dokumente aus PostgreSQL.</p></div>
    <div class="hero-actions"><button class="secondary-action" data-navigate="raw">Rohdateien</button><button class="primary-action" data-navigate="imports">＋ Modell importieren</button></div></header>
    ${dashboard ? dashboardStats() : ""}<section class="content-section"><div class="section-heading"><div><p class="eyebrow">${dashboard ? "ZULETZT BEARBEITET" : "MODELLARCHIV"}</p><h2>${dashboard ? "Aktuelle Modelle" : "Alle Modelle"}</h2></div><div class="section-tools"><span>LIVE-KATALOG</span></div></div><div id="model-tag-filters" class="tag-facets" aria-label="Modelle nach Tag filtern"></div>
    <div id="model-grid-live" class="model-grid"><p class="loading-copy">Modelle werden geladen …</p></div></section></div>`;
  void Promise.all([catalogApi.models(), catalogApi.tags(false)]).then(
    ([models, tags]) => {
      const grid = host.querySelector<HTMLElement>("#model-grid-live");
      if (!grid) return;
      const facets = host.querySelector<HTMLElement>("#model-tag-filters")!;
      let selectedTag = "";
      const render = (): void => {
        const visible = selectedTag ? models.filter((model) => model.tagIds.includes(selectedTag)) : models;
        grid.innerHTML = visible.length > 0 ? visible.map(modelCardMarkup).join("")
          : selectedTag
            ? '<div class="empty-concept"><h2>Keine Modelle mit diesem Tag</h2><p>Filter zurücksetzen oder einen anderen Tag wählen.</p></div>'
            : '<div class="empty-concept"><h2>Noch keine Modelle</h2><p>Importiere ein Modell, um deinen Katalog zu füllen. Indexierte Rohdateien sind nicht automatisch Modelle.</p></div>';
        facets.querySelectorAll<HTMLButtonElement>("[data-tag-facet]").forEach((button) => {
          button.setAttribute("aria-pressed", String(button.dataset.tagFacet === selectedTag));
        });
      };
      const active = tags.items.filter((tag) => tag.active && tag.modelCount > 0);
      facets.innerHTML = active.length === 0 ? "" : `<button type="button" class="tag-chip" data-tag-facet="" aria-pressed="true">Alle · ${models.length}</button>${active.map((tag) => `<button type="button" class="tag-chip" data-tag-facet="${escapeMarkup(tag.id)}" aria-pressed="false" title="${escapeMarkup(tag.name)}">${escapeMarkup(tag.name)} · ${tag.modelCount}</button>`).join("")}`;
      facets.addEventListener("click", (event) => {
        const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-tag-facet]");
        if (!button) return;
        selectedTag = button.dataset.tagFacet === selectedTag ? "" : button.dataset.tagFacet || "";
        render();
      });
      grid.addEventListener("click", (event) => {
        const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-model-tag-filter]");
        if (!button?.dataset.modelTagFilter) return;
        selectedTag = button.dataset.modelTagFilter;
        render();
      });
      render();
      const count = host.querySelector<HTMLElement>("#dashboard-model-count");
      if (count) count.textContent = String(models.length);
      const files = host.querySelector<HTMLElement>("#dashboard-model-files");
      if (files) files.textContent = String(models.reduce((total, model) => total + model.fileCount, 0));
    },
    (error: Error) => showCatalogError(host, error.message),
  );
  if (dashboard) {
    void catalogApi.roots().then((roots) => {
      const element = host.querySelector<HTMLElement>("#indexed-files");
      if (element) element.textContent = String(roots.reduce((sum, root) => sum + root.fileCount - root.missingFileCount, 0));
    });
  }
}

function dashboardStats(): string {
  return `<section class="stat-grid" aria-label="Archivstatistik">
    <article><span class="stat-rune">◇</span><div><strong id="dashboard-model-count">—</strong><small>Modelle</small></div><em>Live</em></article>
    <article><span class="stat-rune">⌁</span><div><strong id="indexed-files">—</strong><small>Indexierte Rohdateien</small></div><em>Live</em></article>
    <article><span class="stat-rune">⬡</span><div><strong id="dashboard-model-files">—</strong><small>Modellverknüpfungen</small></div><em>Stabile IDs</em></article>
    <article><span class="stat-rune">✓</span><div><strong>PostgreSQL</strong><small>Katalogquelle</small></div><em class="success">System bereit</em></article>
  </section>`;
}

function mountModelProblems(host: HTMLElement, modelId: string | null, canEdit: boolean, navigate: Navigate): void {
  host.innerHTML = '<div class="page-scroll"><section class="empty-concept"><span class="forge-rune">◇</span><h2>Probleme werden geladen …</h2></section></div>';
  if (!modelId) {
    showCatalogError(host, "Keine Modell-ID wurde angegeben.");
    return;
  }
  void Promise.all([catalogApi.model(modelId), catalogApi.modelProblems(modelId), identityApi.preferences()]).then(([model, problems, preferences]) => {
    const state = modelProblemState(problems);
    host.innerHTML = `<div class="page-scroll model-problems-page"><nav class="page-crumbs"><button id="back-to-model">Modelle</button><span>/</span><button id="back-to-model-name">${escapeMarkup(model.name)}</button><span>/</span><strong>Probleme</strong></nav>
      <header class="model-header"><div><p class="eyebrow">QUALITÄT & KONVERTIERUNG</p><h1>Probleme · ${escapeMarkup(model.name)}</h1><p>Warnungen, Fehler und Hinweise aus Konvertierung und Inspektion an einer Stelle.</p></div><div class="hero-actions"><span class="problem-indicator tone-${state.tone}"><i></i>${escapeMarkup(state.label)}${state.count ? ` · ${state.count}` : ""}</span><button id="back-to-model-action" class="secondary-action">Zurück zum Modell</button></div></header>
      <div id="model-problem-center"></div></div>`;
    ["#back-to-model", "#back-to-model-name", "#back-to-model-action"].forEach((selector) => host.querySelector(selector)?.addEventListener("click", () => navigate("model", model.id)));
    mountProblemCenter(host.querySelector<HTMLElement>("#model-problem-center")!, model, problems, preferences.problemMinimumSeverity, canEdit);
  }).catch((error: Error) => showCatalogError(host, error.message));
}

function mountThumbnailActions(host: HTMLElement, model: ModelSummary): void {
  host.querySelector<HTMLFormElement>("#thumbnail-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement;
    const status = form.querySelector<HTMLElement>("#thumbnail-status")!;
    const button = form.querySelector<HTMLButtonElement>("button")!;
    const value = String(new FormData(form).get("thumbnail") || "");
    const [kind, candidateId] = value ? value.split("|", 2) : ["default", null];
    button.disabled = true; status.classList.remove("error"); status.textContent = "Auswahl wird gespeichert …";
    try {
      await catalogApi.updateThumbnail(model, kind as ModelSummary["thumbnail"]["kind"], candidateId);
      status.textContent = "Vorschaubild gespeichert.";
      window.location.reload();
    } catch (error) {
      status.classList.add("error"); status.textContent = error instanceof Error ? error.message : "Vorschaubild konnte nicht gespeichert werden."; status.focus();
    } finally { button.disabled = false; }
  });
}

function mountModel(host: HTMLElement, modelId: string | null, cleanup: Array<() => void>, canEdit: boolean, canManageLifecycle: boolean, navigate: Navigate): void {
  let viewer: CadViewer | undefined;
  let pdfViewer: PdfViewer | undefined;
  let stlObserver: IntersectionObserver | undefined;
  const stlTileViewers = new Map<HTMLCanvasElement, { viewer: CadViewer; loaded: boolean }>();
  const visibleStlTiles = new Set<HTMLCanvasElement>();
  let selection = 0;
  let disposed = false;
  cleanup.push(() => {
    disposed = true;
    selection += 1;
    viewer?.dispose();
    pdfViewer?.dispose();
    stlObserver?.disconnect();
    stlTileViewers.forEach(({ viewer: tileViewer }) => tileViewer.dispose());
    stlTileViewers.clear();
  });
  host.innerHTML = '<div class="page-scroll"><section class="empty-concept"><span class="forge-rune">◇</span><h2>Modell wird geladen …</h2></section></div>';
  if (!modelId) {
    showCatalogError(host, "Keine Modell-ID wurde angegeben.");
    return;
  }
  void catalogApi.model(modelId).then(
    async (model) => {
      const [filePage, collectionsCatalog, tagsCatalog, settings, preferences, thumbnailCandidates, slicerTargets, problems] = await Promise.all([
        catalogApi.modelFiles(model.id), catalogApi.collections(), catalogApi.tags(false), identityApi.settings(),
        identityApi.preferences(),
        catalogApi.thumbnailCandidates(model.id), catalogApi.slicerTargets().catch(() => []),
        catalogApi.modelProblems(model.id),
      ]);
      const files = filePage.items;
      const previewProfile = settingString(settings, "previews.defaultProfile", "web") === "fine" ? "fine" : "web";
      const imageFirst = settingString(settings, "thumbnails.defaultSource", "primary-cad") === "image-first";
      const collections = model.collections.join(" · ") || "Ohne Kollektion";
      const problemState = modelProblemState(problems);
      host.innerHTML = `<div class="page-scroll model-page"><nav class="page-crumbs"><button data-navigate="models">Modelle</button><span>/</span><strong>${escapeMarkup(model.name)}</strong></nav>
        <header class="model-header"><div><p class="eyebrow">${escapeMarkup(model.kind.toUpperCase())}</p><h1>${escapeMarkup(model.name)}</h1><p>${escapeMarkup(model.description || "Persistiertes Modell aus der VÖLUND-Bibliothek.")}</p></div>
        <div class="hero-actions"><button class="problem-indicator tone-${problemState.tone}" data-navigate="model-problems" data-model-id="${escapeMarkup(model.id)}"><i></i>${escapeMarkup(problemState.label)}${problemState.count ? ` · ${problemState.count}` : ""}</button><button class="secondary-action" data-navigate="raw">Rohdateien</button>${canEdit ? '<button id="open-model-editor" class="secondary-action">✎ Bearbeiten</button>' : ""}</div></header>
        ${canEdit ? `<section id="model-extension-drop" class="model-extension-drop" tabindex="0"><div><p class="eyebrow">MODELL ERWEITERN</p><h2>Dateien sicher hinzufügen</h2><p>Dateien oder einen Ordner hier ablegen. Metadaten, Tags, Kollektionen und Primärquelle bleiben erhalten.</p></div><div class="import-pickers"><label class="primary-action">Dateien auswählen<input id="model-extension-files" type="file" multiple></label><label class="secondary-action">Ordner auswählen<input id="model-extension-folder" type="file" multiple webkitdirectory></label></div></section>` : ""}
        <section class="model-overview"><div id="model-file-stage-home"><div id="model-file-viewer" class="model-hero-stage"><div class="stage-poster"><span class="forge-rune">◇</span><h2>${escapeMarkup(model.name)}</h2><p>Visuelle Vorschau wird vorbereitet …</p></div></div></div>
        <aside class="model-facts">${thumbnailPanelMarkup(model, thumbnailCandidates, canEdit)}<p class="eyebrow">MODELLDATEN</p><dl><div><dt>Kollektionen</dt><dd>${escapeMarkup(collections)}</dd></div><div><dt>Autor</dt><dd>${escapeMarkup(model.authorName || "Nicht angegeben")}</dd></div><div><dt>Lizenz</dt><dd>${escapeMarkup(model.licenseKind === "not-specified" ? "Nicht angegeben" : model.licenseValue || "Nicht angegeben")}</dd></div><div><dt>Dateien</dt><dd>${model.fileCount}</dd></div><div><dt>Formate</dt><dd>${escapeMarkup(model.formats.join(" · ").toUpperCase() || "—")}</dd></div><div><dt>Revision</dt><dd>${model.revision}</dd></div><div><dt>Aktualisiert</dt><dd>${escapeMarkup(formatUpdated(model.updatedAtUnixMs))}</dd></div></dl>
        <div class="format-pills">${model.tags.map((tag) => `<span>${escapeMarkup(tag)}</span>`).join("") || "<span>OHNE TAGS</span>"}</div>${canEdit ? '<section class="lifecycle-entry"><h3>Sicherer Lebenszyklus</h3><p>Das Modell kann nach einer aktuellen Auswirkungsprüfung aus dem aktiven Katalog entfernt werden. Originaldateien bleiben erhalten.</p><button id="preview-model-removal" class="danger-action" type="button">Modell entfernen …</button><div id="model-lifecycle-state" aria-live="polite"></div></section>' : ""}</aside></section>
        <section class="content-section assembly-structure-section"><div class="section-heading"><div><p class="eyebrow">CAD-STRUKTUR</p><h2>Baugruppen & Teile der primären STEP-Datei</h2></div></div><div id="primary-step-structure" class="primary-step-structure"><p class="loading-copy">STEP-Baugruppenstruktur wird geladen …</p></div></section>
        <section class="content-section model-files-section"><div class="section-heading"><div><p class="eyebrow">PROJEKTINHALT</p><h2>3D-Modelle, Bilder & Dokumente</h2></div><div class="model-file-section-tools"><label>Typ<select id="model-file-filter">${modelFileFilterOptions(files)}</select></label><span id="model-file-count">${files.length} von ${filePage.total} Dateien</span></div></div><div id="model-file-groups">${modelFilesMarkup(files)}</div>${filePage.total > files.length ? `<button id="load-more-model-files" class="secondary-action" type="button">Weitere Dateien laden</button>` : ""}<div id="model-file-context-menu" class="file-context-menu" role="menu" hidden><button type="button" data-context-action="open">Vorschau & Details öffnen</button><a data-context-action="download" download>Original herunterladen</a>${canEdit ? '<button type="button" data-context-action="edit">Dateiangaben bearbeiten</button>' : ""}</div></section>
        <dialog id="model-file-dialog" class="model-file-dialog"><div class="dialog-heading"><div><p class="eyebrow">DATEIDETAIL</p><h2 id="model-file-dialog-title">Datei</h2></div><button id="close-model-file-dialog" class="secondary-action" type="button">Schließen</button></div><div id="model-file-dialog-preview"></div><div id="model-file-dialog-details"></div></dialog>
        <section class="content-section"><div class="section-heading"><div><p class="eyebrow">ÄNDERUNGSVERLAUF</p><h2>Kataloghistorie</h2></div></div><div id="model-history"><p class="loading-copy">Änderungsverlauf wird geladen …</p></div></section></div>`;
      mountModelHistory(host, model.id);
      const prepareLifecycle = async (action: import("./types").LifecycleAction, targetId: string, revision: number, parentId: string | null, target: HTMLElement): Promise<void> => {
        target.textContent = "Auswirkungen werden geprüft …";
        try {
          const plan = await catalogApi.previewLifecycle(action, targetId, revision, parentId);
          target.innerHTML = `<form class="lifecycle-confirmation"><h3>Auswirkungen bestätigen</h3><pre></pre><label>Zur Bestätigung exakt eingeben <strong>${escapeMarkup(plan.confirmation)}</strong><input name="confirmation" autocomplete="off" required></label><div class="dialog-actions"><button type="submit" class="danger-action">Jetzt anwenden</button></div><p class="form-message" aria-live="polite"></p></form>`;
          target.querySelector("pre")!.textContent = JSON.stringify(plan.impact, null, 2);
          const form = target.querySelector<HTMLFormElement>("form")!;
          form.querySelector<HTMLInputElement>("input")!.focus();
          form.addEventListener("submit", async (event) => {
            event.preventDefault();
            const button = form.querySelector<HTMLButtonElement>("button")!;
            const status = form.querySelector<HTMLElement>(".form-message")!;
            button.disabled = true; status.textContent = "Lebenszyklusaktion wird angewendet …";
            try {
              await catalogApi.applyLifecycle(plan, String(new FormData(form).get("confirmation") || ""));
              status.textContent = "Aktion abgeschlossen. Die Ansicht wird aktualisiert …";
              window.location.reload();
            } catch (error) {
              button.disabled = false; status.classList.add("error");
              status.textContent = error instanceof Error ? error.message : "Die Aktion konnte nicht angewendet werden.";
            }
          });
        } catch (error) {
          target.textContent = error instanceof Error ? error.message : "Auswirkungen konnten nicht geladen werden.";
        }
      };
      host.querySelector<HTMLButtonElement>("#preview-model-removal")?.addEventListener("click", () => {
        void prepareLifecycle("model.remove", model.id, model.revision, null, host.querySelector<HTMLElement>("#model-lifecycle-state")!);
      });
      if (canEdit) {
        mountModelEditor(host, model, collectionsCatalog, files, tagsCatalog.items, () => window.location.reload());
        mountThumbnailActions(host, model);
        const startExtension = (selected: ArrayLike<File>): void => {
          if (selected.length === 0) return;
          queueModelExtension(model, selected);
          navigate("imports");
        };
        host.querySelector<HTMLInputElement>("#model-extension-files")?.addEventListener("change", (event) => startExtension((event.currentTarget as HTMLInputElement).files!));
        host.querySelector<HTMLInputElement>("#model-extension-folder")?.addEventListener("change", (event) => startExtension((event.currentTarget as HTMLInputElement).files!));
        const extensionDrop = host.querySelector<HTMLElement>("#model-extension-drop");
        extensionDrop?.addEventListener("dragover", (event) => { event.preventDefault(); extensionDrop.classList.add("dragging"); });
        extensionDrop?.addEventListener("dragleave", () => extensionDrop.classList.remove("dragging"));
        extensionDrop?.addEventListener("drop", (event) => {
          event.preventDefault();
          extensionDrop.classList.remove("dragging");
          if (event.dataTransfer?.files) startExtension(event.dataTransfer.files);
        });
      }
      const primaryStep = files.find((file) => file.primary && file.format === "step" && !file.missing);
      void renderPrimaryStepStructure(host, primaryStep, () => disposed, {
        select: (name) => viewer?.selectObject(name), visible: (name, visible) => viewer?.setObjectVisible(name, visible),
        isolate: (name) => viewer?.isolateObject(name), reset: () => viewer?.resetVisibility(),
      }).catch((error: Error) => {
        const target = host.querySelector<HTMLElement>("#primary-step-structure");
        if (target && !disposed) target.innerHTML = viewerMessage("STEP-Struktur nicht verfügbar", error.message);
      });
      const byId = new Map(files.map((file) => [file.id, file]));
      stlObserver = new IntersectionObserver((entries) => entries.forEach((entry) => {
        const canvas = entry.target as HTMLCanvasElement;
        if (!entry.isIntersecting) {
          visibleStlTiles.delete(canvas);
          const active = stlTileViewers.get(canvas);
          if (active?.loaded) {
            active.viewer.dispose();
            stlTileViewers.delete(canvas);
          }
          return;
        }
        visibleStlTiles.add(canvas);
        if (stlTileViewers.has(canvas)) return;
        const file = byId.get(canvas.dataset.stlPreview!);
        if (!file) return;
        const tileViewer = new CadViewer(canvas, preferences);
        const active = { viewer: tileViewer, loaded: false };
        stlTileViewers.set(canvas, active);
        const state = canvas.parentElement?.querySelector<HTMLElement>("[data-stl-state]");
        void tileViewer.load(catalogApi.sourceContentUrl(file.id), "stl").then(() => {
          active.loaded = true;
          if (state) state.textContent = "ZIEHEN · DREHEN";
          if (!visibleStlTiles.has(canvas)) {
            tileViewer.dispose();
            stlTileViewers.delete(canvas);
          }
        }).catch((error: Error) => {
          active.loaded = true;
          if (state) state.textContent = `VORSCHAU FEHLERHAFT · ${error.message}`;
        });
      }), { rootMargin: "180px" });
      const observeStlCanvases = (): void => {
        stlObserver?.disconnect();
        visibleStlTiles.clear();
        stlTileViewers.forEach(({ viewer: tileViewer }) => tileViewer.dispose());
        stlTileViewers.clear();
        host.querySelectorAll<HTMLCanvasElement>("[data-stl-preview]").forEach((canvas) => {
          canvas.addEventListener("click", (event) => event.stopPropagation());
          canvas.addEventListener("pointerdown", (event) => event.stopPropagation());
          stlObserver?.observe(canvas);
        });
      };
      observeStlCanvases();
      const showModelFile = async (file: ModelFile): Promise<void> => {
        const token = ++selection;
        viewer?.dispose();
        viewer = undefined;
        pdfViewer?.dispose();
        pdfViewer = undefined;
        host.querySelectorAll("[data-model-file]").forEach((card) => card.classList.toggle("selected", (card as HTMLElement).dataset.modelFile === file.id));
        const stage = host.querySelector<HTMLElement>("#model-file-viewer");
        if (!stage || disposed) return;
        const name = file.path.split("/").at(-1) || file.path;
        const contentUrl = catalogApi.sourceContentUrl(file.id);
        if (file.missing) {
          stage.innerHTML = viewerMessage(name, "Die Datei ist am katalogisierten Speicherort nicht verfügbar.");
          return;
        }
        const extension = file.path.split(".").at(-1)?.toLowerCase();
        if (file.role === "image") {
          stage.innerHTML = `<img class="model-media-image" src="${contentUrl}" alt="${escapeMarkup(name)}"><div class="model-media-caption"><strong>${escapeMarkup(name)}</strong><span>BILD · ${formatBytes(file.byteSize)}</span></div>`;
          return;
        }
        if (extension === "pdf") {
          stage.innerHTML = pdfViewerMarkup(name, formatBytes(file.byteSize), contentUrl);
          pdfViewer = new PdfViewer(stage, contentUrl);
          await pdfViewer.load();
          return;
        }
        if (file.role === "document" || ["md", "txt", "cfg", "yml", "yaml", "json"].includes(extension || "")) {
          stage.innerHTML = `<iframe class="model-document-frame" src="${contentUrl}" title="${escapeMarkup(name)}"></iframe><div class="model-media-caption"><strong>${escapeMarkup(name)}</strong><span>DOKUMENT · ${formatBytes(file.byteSize)}</span><a href="${contentUrl}" target="_blank" rel="noopener">Separat öffnen ↗</a></div>`;
          return;
        }
        if (extension === "stl") {
          stage.innerHTML = `<canvas id="model-hero-canvas"></canvas><div id="model-hero-state" class="viewer-state visible">STL wird direkt geladen …</div><div class="viewer-hint">Ziehen · Drehen &nbsp; Scrollen · Zoomen</div><div class="model-media-caption"><strong>${escapeMarkup(name)}</strong><span>STL · ${formatBytes(file.byteSize)}</span></div>`;
          const canvas = stage.querySelector<HTMLCanvasElement>("#model-hero-canvas")!;
          const state = stage.querySelector<HTMLElement>("#model-hero-state")!;
          viewer = new CadViewer(canvas, preferences);
          await viewer.load(contentUrl, "stl", model.viewerRotation);
          if (token === selection) state.classList.remove("visible");
          return;
        }
        if (file.format) {
          stage.innerHTML = `<canvas id="model-hero-canvas"></canvas><div id="model-hero-state" class="viewer-state visible">3D-Vorschau wird gesucht …</div><div class="viewer-hint">Ziehen · Drehen &nbsp; Scrollen · Zoomen</div><div class="model-media-caption"><strong>${escapeMarkup(name)}</strong><span>${escapeMarkup(file.format.toUpperCase())} · ${formatBytes(file.byteSize)}</span></div>`;
          const canvas = stage.querySelector<HTMLCanvasElement>("#model-hero-canvas")!;
          const state = stage.querySelector<HTMLElement>("#model-hero-state")!;
          viewer = new CadViewer(canvas, preferences);
          let artifact = await previewArtifact(file.id);
          if (!artifact) {
            state.textContent = "3D-Vorschau wurde eingereiht …";
            await catalogApi.enqueuePreview(file.id, previewProfile);
            artifact = await waitForPreview(file.id, () => disposed || token !== selection);
          }
          if (disposed || token !== selection) return;
          if (!artifact) {
            state.textContent = "3D-Vorschau wird im Hintergrund erzeugt. Diese Ansicht aktualisiert sich weiter.";
            return;
          }
          state.textContent = `3D-Modell wird geladen · ${formatBytes(artifact.byteSize)}`;
          await viewer.load(artifact.url, "glb", model.viewerRotation);
          if (token === selection) state.classList.remove("visible");
          return;
        }
        stage.innerHTML = `<div class="model-generic-preview"><span>${fileGlyph(file)}</span><h2>${escapeMarkup(name)}</h2><p>Diese Datei besitzt keinen eingebetteten Viewer.</p><a class="primary-action" href="${contentUrl}" target="_blank" rel="noopener">Datei öffnen / herunterladen</a></div>`;
      };
      const openModelFile = (file: ModelFile): void => {
        void showModelFile(file).catch((error: Error) => {
          const stage = host.querySelector<HTMLElement>("#model-file-viewer");
          if (stage && !disposed) stage.innerHTML = viewerMessage(file.path.split("/").at(-1) || file.path, error.message);
        });
      };
      const dialog = host.querySelector<HTMLDialogElement>("#model-file-dialog")!;
      const stage = host.querySelector<HTMLElement>("#model-file-viewer")!;
      const stageHome = host.querySelector<HTMLElement>("#model-file-stage-home")!;
      const closeDialog = (): void => { dialog.close(); };
      host.querySelector("#close-model-file-dialog")?.addEventListener("click", closeDialog);
      dialog.addEventListener("cancel", (event) => { event.preventDefault(); closeDialog(); });
      dialog.addEventListener("close", () => {
        restoreViewerStage(stageHome, stage, () => viewer?.refreshLayout());
      });
      const openFileDetail = (file: ModelFile): void => {
        host.querySelector<HTMLElement>("#model-file-dialog-title")!.textContent = file.caption || file.path.split("/").at(-1) || file.path;
        host.querySelector<HTMLElement>("#model-file-dialog-details")!.innerHTML = fileDetailMarkup(file, canEdit, slicerTargets);
        bindCatalogHistories(dialog);
        host.querySelector<HTMLElement>("#model-file-dialog-preview")!.append(stage);
        dialog.showModal();
        openModelFile(file);
        const form = dialog.querySelector<HTMLFormElement>("#model-file-metadata-form");
        dialog.querySelector<HTMLButtonElement>("[data-set-primary-file]")?.addEventListener("click", async (event) => {
          const button = event.currentTarget as HTMLButtonElement;
          button.disabled = true;
          button.textContent = "Primärdatei wird gesetzt …";
          try {
            await catalogApi.updateModelPrimary(model, file.id);
            window.location.reload();
          } catch (error) {
            button.disabled = false;
            button.textContent = error instanceof Error ? error.message : "Primärdatei konnte nicht gesetzt werden.";
          }
        });
        form?.addEventListener("submit", async (event) => {
          event.preventDefault();
          const data = new FormData(form);
          const status = form.querySelector<HTMLElement>(".form-message")!;
          const button = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
          button.disabled = true; status.textContent = "Dateiangaben werden gespeichert …";
          try {
            const updated = await catalogApi.updateModelFile(model.id, file.id, {
              expectedRevision: file.revision,
              caption: String(data.get("caption") || ""), description: String(data.get("description") || ""),
              notes: String(data.get("notes") || ""), printable: data.has("printable"), printed: data.has("printed"),
              preSupported: data.has("preSupported"), upAxis: String(data.get("upAxis") || "") as ModelFile["upAxis"] || null,
              supportHint: String(data.get("supportHint") || ""),
              orientation: ["orientationX", "orientationY", "orientationZ"].map((key) => Number(data.get(key))) as [number, number, number],
            });
            Object.assign(file, updated); status.textContent = "Dateiangaben gespeichert.";
          } catch (error) {
            status.classList.add("error"); status.textContent = error instanceof Error ? error.message : "Dateiangaben konnten nicht gespeichert werden.";
          } finally { button.disabled = false; }
        });
        dialog.querySelectorAll<HTMLButtonElement>("[data-slicer-target]").forEach((slicerButton) => slicerButton.addEventListener("click", async () => {
          const status = dialog.querySelector<HTMLElement>("#slicer-status")!;
          slicerButton.disabled = true; status.textContent = "Slicer-Übergabe wird vorbereitet …";
          try {
            const handoff = await catalogApi.createSlicerHandoff(model.id, file.id, slicerButton.dataset.slicerTarget!);
            status.innerHTML = slicerHandoffStatusMarkup(handoff.downloadUrl);
            window.location.href = handoff.launchUrl;
          } catch (error) {
            status.textContent = error instanceof Error ? error.message : "Slicer konnte nicht geöffnet werden.";
          } finally { slicerButton.disabled = false; }
        }));
      };
      const fileSection = host.querySelector<HTMLElement>(".model-files-section")!;
      const contextMenu = host.querySelector<HTMLElement>("#model-file-context-menu")!;
      let contextFile: ModelFile | undefined;
      let contextTrigger: HTMLElement | undefined;
      const closeContextMenu = (restoreFocus = false): void => {
        hideFileContextMenu(contextMenu, contextTrigger, restoreFocus);
        contextFile = undefined;
        contextTrigger = undefined;
      };
      const openContextMenu = (file: ModelFile, x: number, y: number, trigger?: HTMLElement): void => {
        contextFile = file;
        contextTrigger = trigger;
        const downloadLink = contextMenu.querySelector<HTMLAnchorElement>('[data-context-action="download"]')!;
        downloadLink.hidden = file.missing;
        downloadLink.href = file.missing ? "" : catalogApi.sourceDownloadUrl(file.id);
        downloadLink.download = file.path.split("/").at(-1) || "download";
        contextMenu.hidden = false;
        const width = contextMenu.offsetWidth;
        const height = contextMenu.offsetHeight;
        contextMenu.style.left = `${Math.max(12, Math.min(x, window.innerWidth - width - 12))}px`;
        contextMenu.style.top = `${Math.max(12, Math.min(y, window.innerHeight - height - 12))}px`;
        contextMenu.querySelector<HTMLElement>("button:not([hidden]), a:not([hidden])")?.focus();
      };
      fileSection.addEventListener("contextmenu", (event) => {
        const card = (event.target as HTMLElement).closest<HTMLElement>("[data-model-file]");
        const file = card ? byId.get(card.dataset.modelFile!) : undefined;
        if (!file) return;
        event.preventDefault();
        openContextMenu(file, event.clientX, event.clientY);
      });
      fileSection.addEventListener("click", (event) => {
        const target = event.target as HTMLElement;
        const menuTrigger = target.closest<HTMLElement>("[data-file-menu]");
        if (menuTrigger) {
          event.stopPropagation();
          const file = byId.get(menuTrigger.dataset.fileMenu!);
          if (file) {
            const bounds = menuTrigger.getBoundingClientRect();
            openContextMenu(file, bounds.right, bounds.bottom + 6, menuTrigger);
          }
          return;
        }
        const action = target.closest<HTMLElement>("[data-context-action]");
        if (action && contextFile) {
          if (action.dataset.contextAction !== "download") {
            event.preventDefault();
            openFileDetail(contextFile);
          }
          closeContextMenu();
          return;
        }
        const card = target.closest<HTMLElement>("[data-open-file]");
        const file = card ? byId.get(card.dataset.openFile!) : undefined;
        if (file) openFileDetail(file);
      });
      const handleContextEscape = (event: KeyboardEvent): void => { if (event.key === "Escape") closeContextMenu(true); };
      const handleContextDismiss = (): void => closeContextMenu();
      document.addEventListener("click", handleContextDismiss);
      document.addEventListener("keydown", handleContextEscape);
      cleanup.push(() => document.removeEventListener("click", handleContextDismiss));
      cleanup.push(() => document.removeEventListener("keydown", handleContextEscape));
      const applyFileFilter = (): void => {
        const filter = host.querySelector<HTMLSelectElement>("#model-file-filter")?.value || "all";
        let visible = 0;
        host.querySelectorAll<HTMLElement>("[data-model-file]").forEach((card) => {
          card.hidden = filter !== "all" && card.dataset.fileFilter !== filter;
          if (!card.hidden) visible += 1;
        });
        host.querySelectorAll<HTMLElement>(".model-file-group").forEach((group) => {
          group.hidden = !group.querySelector("[data-model-file]:not([hidden])");
        });
        host.querySelector<HTMLElement>("#model-file-count")!.textContent = filter === "all" ? `${files.length} von ${filePage.total} Dateien` : `${visible} gefilterte Dateien`;
      };
      host.querySelector<HTMLSelectElement>("#model-file-filter")?.addEventListener("change", applyFileFilter);
      host.querySelector<HTMLButtonElement>("#load-more-model-files")?.addEventListener("click", async (event) => {
        const button = event.currentTarget as HTMLButtonElement;
        button.disabled = true; button.textContent = "Weitere Dateien werden geladen …";
        try {
          const next = await catalogApi.modelFiles(model.id, files.length);
          files.push(...next.items);
          next.items.forEach((file) => byId.set(file.id, file));
          host.querySelector<HTMLElement>("#model-file-groups")!.innerHTML = modelFilesMarkup(files);
          host.querySelector<HTMLSelectElement>("#model-file-filter")!.innerHTML = modelFileFilterOptions(files);
          host.querySelector<HTMLElement>("#model-file-count")!.textContent = `${files.length} von ${next.total} Dateien`;
          observeStlCanvases();
          applyFileFilter();
          if (files.length >= next.total) button.remove();
          else { button.disabled = false; button.textContent = "Weitere Dateien laden"; }
        } catch (error) {
          button.disabled = false;
          button.textContent = error instanceof Error ? `Erneut versuchen · ${error.message}` : "Erneut versuchen";
        }
      });
      const initial = preferredModelFile(files, imageFirst);
      if (initial && preferences.previewAutoLoad !== "manual") openModelFile(initial);
      else if (preferences.previewAutoLoad === "manual") {
        const stage = host.querySelector<HTMLElement>("#model-file-viewer");
        if (stage) stage.innerHTML = '<div class="stage-poster"><span class="forge-rune">◇</span><h2>Vorschau bereit</h2><p>Automatisches Laden ist für dieses Konto deaktiviert. Öffne eine Datei bewusst.</p></div>';
      }
    },
  ).catch((error: Error) => showCatalogError(host, error.message));
}

function viewerMessage(title: string, message: string): string {
  return `<div class="model-generic-preview"><span>!</span><h2>${escapeMarkup(title)}</h2><p>${escapeMarkup(message)}</p></div>`;
}

async function previewArtifact(fileId: string, kind: Artifact["kind"] = "preview-glb") {
  const previews = await catalogApi.previews(fileId);
  return previews.items.find((preview) => preview.status === "ready")?.artifacts.find((artifact) => artifact.kind === kind);
}

async function waitForPreview(fileId: string, cancelled: () => boolean) {
  return waitForArtifact(fileId, "preview-glb", cancelled);
}

async function renderPrimaryStepStructure(
  host: HTMLElement,
  primary: ModelFile | undefined,
  cancelled: () => boolean,
  actions: import("./assembly-tree").AssemblyViewerActions,
): Promise<void> {
  const target = host.querySelector<HTMLElement>("#primary-step-structure");
  if (!target || cancelled()) return;
  if (!primary) {
    target.innerHTML = '<div class="empty-concept"><h2>Keine primäre STEP-Datei</h2><p>Die CAD-Baugruppe wird ausschließlich aus der als primär markierten STEP-Datei gelesen.</p></div>';
    return;
  }
  let artifact = await previewArtifact(primary.id, "assembly-manifest");
  if (!artifact) {
    target.innerHTML = '<p class="loading-copy">STEP-Baugruppenstruktur wird erzeugt …</p>';
    await catalogApi.enqueuePreview(primary.id);
    artifact = await waitForArtifact(primary.id, "assembly-manifest", cancelled);
  }
  if (!artifact || cancelled()) {
    if (!cancelled()) target.innerHTML = '<p class="loading-copy">Die STEP-Struktur wird weiter im Hintergrund erzeugt. Bitte die Seite später neu laden.</p>';
    return;
  }
  const manifest = parseAssemblyManifest(await requestJson<unknown>(artifact.url));
  if (!cancelled()) {
    target.innerHTML = assemblyTreeMarkup(manifest, primary.path.split("/").at(-1) || primary.path);
    mountAssemblyTree(target, manifest, actions);
  }
}

export async function waitForArtifact(fileId: string, kind: Artifact["kind"], cancelled: () => boolean) {
  for (let attempt = 0; attempt < 90 && !cancelled(); attempt += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 2000));
    if (cancelled()) return undefined;
    const artifact = await previewArtifact(fileId, kind);
    if (artifact) return artifact;
  }
  return undefined;
}

function showCatalogError(host: HTMLElement, message: string): void {
  const target = host.querySelector<HTMLElement>("#model-grid-live, #metadata-catalog") ?? host;
  target.innerHTML = `<div class="empty-concept"><h2>Katalog nicht verfügbar</h2><p>${escapeMarkup(message)}</p></div>`;
}

function formatUpdated(timestamp: number): string {
  return new Intl.DateTimeFormat("de-DE", { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp));
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}
