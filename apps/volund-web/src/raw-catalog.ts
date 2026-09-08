import { catalogApi } from "./api";
import { folderCrumbs, readCatalogLocation, writeCatalogLocation } from "./catalog-state";
import type { CatalogLocation } from "./catalog-state";
import { fileName, formatBytes, formatDate, newestReadyPreview, parentPath, previewLabel } from "./format";
import { ancestorPaths, folderCountLabel, treeNodeKey } from "./folder-tree";
import { moveTargetPath, parentDirectory } from "./move-target";
import type { CadFile, Folder, LibraryRoot, Page, Preview } from "./types";
import { CadViewer } from "./viewer";

const PAGE_SIZE = 100;

export function mountRawCatalog(host: HTMLElement): () => void {
host.innerHTML = `
  <div class="raw-shell">
    <aside class="roots-panel">
      <div class="section-title"><span>Bibliotheken</span><span id="root-count">—</span></div>
      <nav id="roots" aria-label="Bibliotheken"></nav>
      <div class="sovereignty"><span>CONTROLLED STORAGE</span><p>Nur bestätigte Ablageaktionen dürfen Originaldateien verschieben.</p></div>
    </aside>
    <main class="catalog-panel">
      <div class="catalog-head">
        <div class="catalog-title"><button id="tree-toggle" class="tree-toggle-mobile" aria-label="Ordnerbaum öffnen">☰</button><div><p class="eyebrow" id="root-eyebrow">ARCHIV</p><h1 id="root-title">CAD-Bibliothek</h1></div></div>
        <label class="search"><span>⌕</span><input id="search" type="search" placeholder="Unterhalb dieses Ordners suchen" /></label>
      </div>
      <div class="catalog-tools">
        <nav id="breadcrumbs" class="breadcrumbs" aria-label="Ordnerpfad"></nav>
        <div class="filters">
          <select id="format-filter" aria-label="Format filtern"><option value="">Alle Formate</option><option>STEP</option><option>IGES</option><option>BREP</option><option>STL</option><option>3MF</option><option>OBJ</option><option>PLY</option><option>GLTF</option><option>GLB</option></select>
          <select id="sort" aria-label="Sortierung"><option value="path:asc">Name A–Z</option><option value="path:desc">Name Z–A</option><option value="modified:desc">Neueste zuerst</option><option value="modified:asc">Älteste zuerst</option><option value="size:desc">Größte zuerst</option><option value="size:asc">Kleinste zuerst</option><option value="format:asc">Nach Format</option></select>
        </div>
      </div>
      <div class="table-head"><span>Name</span><span>Format</span><span>Größe</span><span>Geändert</span></div>
      <div id="files" class="file-list"><div class="empty">Bibliotheken werden geladen …</div></div>
      <div class="pager"><button id="previous" disabled>← Zurück</button><span id="page-info">—</span><button id="next" disabled>Weiter →</button></div>
    </main>
    <section class="detail-panel" id="details">
      <div class="detail-empty"><span class="forge-rune">◇</span><h2>Bauteil auswählen</h2><p>Wähle eine CAD-Datei, um Metadaten und 3D-Vorschau zu öffnen.</p></div>
    </section>
  </div>
  <dialog id="move-dialog" class="move-dialog">
    <div class="move-dialog-head"><div><p class="eyebrow">KONTROLLIERTE ABLAGE</p><h2>Datei verschieben</h2></div><button id="move-close" aria-label="Dialog schließen">×</button></div>
    <p id="move-source" class="move-source"></p>
    <nav id="move-breadcrumbs" class="breadcrumbs move-breadcrumbs" aria-label="Zielordner"></nav>
    <div id="move-folders" class="move-folders"></div>
    <p id="move-target" class="move-target"></p>
    <p id="move-error" class="move-error" role="alert"></p>
    <div class="move-actions"><button id="move-cancel">Abbrechen</button><button id="move-confirm" class="primary">Hierher verschieben</button></div>
  </dialog>`;

const rootsElement = document.querySelector<HTMLElement>("#roots")!;
const filesElement = document.querySelector<HTMLElement>("#files")!;
const detailsElement = document.querySelector<HTMLElement>("#details")!;
const breadcrumbsElement = document.querySelector<HTMLElement>("#breadcrumbs")!;
const searchElement = document.querySelector<HTMLInputElement>("#search")!;
const formatElement = document.querySelector<HTMLSelectElement>("#format-filter")!;
const sortElement = document.querySelector<HTMLSelectElement>("#sort")!;
const previousButton = document.querySelector<HTMLButtonElement>("#previous")!;
const nextButton = document.querySelector<HTMLButtonElement>("#next")!;
const rawShell = host.querySelector<HTMLElement>(".raw-shell")!;
const treeToggle = host.querySelector<HTMLButtonElement>("#tree-toggle")!;
const moveDialog = host.querySelector<HTMLDialogElement>("#move-dialog")!;
const moveFoldersElement = host.querySelector<HTMLElement>("#move-folders")!;
const moveBreadcrumbsElement = host.querySelector<HTMLElement>("#move-breadcrumbs")!;
const moveConfirmButton = host.querySelector<HTMLButtonElement>("#move-confirm")!;
let roots: LibraryRoot[] = [];
let selectedRoot: LibraryRoot | undefined;
let page: Page<CadFile> = { items: [], limit: PAGE_SIZE, offset: 0, total: 0 };
let folders: Folder[] = [];
let location = readCatalogLocation(window.location.search, PAGE_SIZE);
let viewer: CadViewer | undefined;
let searchTimer = 0;
let moveSource: CadFile | undefined;
let moveDirectory = "";
const treeChildren = new Map<string, Folder[]>();
const treeExpanded = new Set<string>();
const treeLoading = new Set<string>();

function setText(selector: string, value: string): void {
  const element = document.querySelector<HTMLElement>(selector);
  if (element) element.textContent = value;
}

function syncUrl(replace = false): void {
  const url = writeCatalogLocation(location);
  window.history[replace ? "replaceState" : "pushState"]({}, "", url);
}

function showError(message: string): void {
  filesElement.replaceChildren();
  const error = document.createElement("div");
  error.className = "empty error";
  error.textContent = message;
  filesElement.append(error);
}

function renderRoots(): void {
  rootsElement.replaceChildren();
  for (const root of roots) {
    const branch = document.createElement("div");
    branch.className = "tree-root";
    const row = document.createElement("div");
    row.className = "tree-row tree-root-row";
    const key = treeNodeKey(root.key, "");
    row.append(treeToggleButton(root, "", key));
    const button = document.createElement("button");
    button.className = `tree-label tree-root-label${root.key === selectedRoot?.key && location.directory === "" ? " active" : ""}`;
    button.innerHTML = `<span class="root-glyph">⬡</span><span><strong></strong><small></small></span><b></b>`;
    button.querySelector("strong")!.textContent = root.name;
    button.querySelector("small")!.textContent = root.latestScanStatus ?? "Noch nicht gescannt";
    button.querySelector("b")!.textContent = String(root.fileCount - root.missingFileCount);
    button.addEventListener("click", () => void selectRoot(root));
    row.append(button);
    branch.append(row);
    if (treeExpanded.has(key)) {
      const children = document.createElement("div");
      children.className = "tree-children";
      appendFolderNodes(children, root, "", 1);
      branch.append(children);
    }
    rootsElement.append(branch);
  }
}

function treeToggleButton(root: LibraryRoot, directory: string, key: string): HTMLButtonElement {
  const toggle = document.createElement("button");
  const loaded = treeChildren.has(key);
  const hasChildren = !loaded || (treeChildren.get(key)?.length ?? 0) > 0;
  toggle.className = `tree-toggle${hasChildren ? "" : " leaf"}`;
  toggle.textContent = hasChildren ? (treeExpanded.has(key) ? "⌄" : "›") : "·";
  toggle.ariaLabel = hasChildren
    ? `${directory || root.name} ${treeExpanded.has(key) ? "einklappen" : "aufklappen"}`
    : `${directory || root.name} ohne Unterordner`;
  toggle.disabled = !hasChildren;
  toggle.addEventListener("click", () => void toggleTreeNode(root, directory));
  return toggle;
}

function appendFolderNodes(container: HTMLElement, root: LibraryRoot, parent: string, depth: number): void {
  const parentKey = treeNodeKey(root.key, parent);
  if (treeLoading.has(parentKey)) {
    const loading = document.createElement("span");
    loading.className = "tree-loading";
    loading.textContent = "Ordner werden geladen …";
    container.append(loading);
    return;
  }
  for (const folder of treeChildren.get(parentKey) ?? []) {
    const key = treeNodeKey(root.key, folder.path);
    const row = document.createElement("div");
    row.className = "tree-row tree-folder-row";
    row.style.setProperty("--tree-depth", String(depth));
    row.append(treeToggleButton(root, folder.path, key));
    const button = document.createElement("button");
    button.className = `tree-label tree-folder-label${root.key === selectedRoot?.key && folder.path === location.directory ? " active" : ""}`;
    button.innerHTML = `<span class="tree-folder-glyph">▱</span><span></span><b></b>`;
    button.querySelector("span:nth-child(2)")!.textContent = folder.name;
    button.querySelector("b")!.textContent = String(folder.fileCount);
    button.title = `${folder.path} · ${folderCountLabel(folder.fileCount)}`;
    button.addEventListener("click", () => void openFolder(folder.path));
    row.append(button);
    container.append(row);
    if (treeExpanded.has(key)) appendFolderNodes(container, root, folder.path, depth + 1);
  }
}

async function toggleTreeNode(root: LibraryRoot, directory: string): Promise<void> {
  const key = treeNodeKey(root.key, directory);
  if (treeExpanded.has(key)) {
    treeExpanded.delete(key);
    renderRoots();
    return;
  }
  treeExpanded.add(key);
  renderRoots();
  await loadTreeChildren(root.key, directory);
}

async function loadTreeChildren(rootKey: string, directory: string): Promise<void> {
  const key = treeNodeKey(rootKey, directory);
  if (treeChildren.has(key) || treeLoading.has(key)) return;
  treeLoading.add(key);
  renderRoots();
  try {
    const items: Folder[] = [];
    let offset = 0;
    let total = 0;
    do {
      const page = await catalogApi.folders(rootKey, {
        directory,
        query: "",
        format: "",
        sort: "path",
        direction: "asc",
        offset,
        limit: 200,
      });
      items.push(...page.items);
      total = page.total;
      if (page.items.length === 0) break;
      offset += page.items.length;
    } while (offset < total);
    treeChildren.set(key, items);
  } finally {
    treeLoading.delete(key);
    renderRoots();
  }
}

async function revealDirectory(rootKey: string, directory: string): Promise<void> {
  for (const path of ancestorPaths(directory)) {
    treeExpanded.add(treeNodeKey(rootKey, path));
    await loadTreeChildren(rootKey, path);
  }
}

function renderBreadcrumbs(): void {
  breadcrumbsElement.replaceChildren();
  for (const [index, crumb] of folderCrumbs(location.directory).entries()) {
    if (index > 0) breadcrumbsElement.append("/");
    const button = document.createElement("button");
    button.textContent = crumb.name;
    button.className = crumb.path === location.directory ? "current" : "";
    button.addEventListener("click", () => void openFolder(crumb.path));
    breadcrumbsElement.append(button);
  }
}

function folderRow(folder: Folder): HTMLButtonElement {
  const button = document.createElement("button");
  button.className = "file-row folder-row";
  button.innerHTML = `<span class="file-cell"><i>◇</i><span><strong></strong><small></small></span></span><span class="format">ORDNER</span><span></span><span></span>`;
  button.querySelector("strong")!.textContent = folder.name;
  button.querySelector("small")!.textContent = `${folderCountLabel(folder.fileCount)} enthalten`;
  const columns = button.querySelectorAll<HTMLElement>(":scope > span");
  columns[2]!.textContent = "—";
  columns[3]!.textContent = "Öffnen →";
  button.addEventListener("click", () => void openFolder(folder.path));
  return button;
}

function fileRow(file: CadFile): HTMLButtonElement {
  const button = document.createElement("button");
  button.className = `file-row${file.id === location.file ? " selected" : ""}`;
  button.innerHTML = `<span class="file-cell"><i></i><span><strong></strong><small></small></span></span><span class="format"></span><span></span><span></span>`;
  button.querySelector("i")!.textContent = (file.format ?? "cad").slice(0, 3).toUpperCase();
  button.querySelector("strong")!.textContent = fileName(file.path);
  button.querySelector("small")!.textContent = parentPath(file.path);
  const columns = button.querySelectorAll<HTMLElement>(":scope > span");
  columns[1]!.textContent = file.format?.toUpperCase() ?? "CAD";
  columns[2]!.textContent = formatBytes(file.byteSize);
  columns[3]!.textContent = formatDate(file.modifiedAtUnixMs);
  button.addEventListener("click", () => void selectFile(file));
  return button;
}

function renderEntries(): void {
  filesElement.replaceChildren(...folders.map(folderRow), ...page.items.map(fileRow));
  if (folders.length + page.items.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = location.query ? "Keine Datei entspricht dieser Suche." : "Dieser Ordner ist leer.";
    filesElement.append(empty);
  }
  const first = page.total === 0 ? 0 : page.offset + 1;
  const last = Math.min(page.offset + page.items.length, page.total);
  setText("#page-info", `${first}–${last} von ${page.total} Dateien`);
  previousButton.disabled = page.offset === 0;
  nextButton.disabled = page.offset + page.limit >= page.total;
}

async function loadCatalog(replaceUrl = false): Promise<void> {
  if (!selectedRoot) return;
  renderBreadcrumbs();
  renderRoots();
  syncUrl(replaceUrl);
  filesElement.innerHTML = '<div class="empty">Katalog wird geladen …</div>';
  try {
    const folderOptions = { ...location, offset: 0, limit: 200 };
    const [loadedPage, loadedFolders] = await Promise.all([
      catalogApi.files(selectedRoot.key, location),
      catalogApi.folders(selectedRoot.key, folderOptions),
    ]);
    page = loadedPage;
    folders = loadedFolders.items;
    setText("#root-eyebrow", location.query ? `${page.total} SUCHTREFFER` : `${selectedRoot.fileCount - selectedRoot.missingFileCount} VERFÜGBARE DATEIEN`);
    renderEntries();
    const selected = page.items.find((file) => file.id === location.file);
    if (selected) {
      await selectFile(selected, true);
    } else {
      viewer?.dispose();
      viewer = undefined;
      detailsElement.innerHTML = '<div class="detail-empty"><span class="forge-rune">◇</span><h2>Bauteil auswählen</h2><p>Wähle eine CAD-Datei, um Metadaten und 3D-Vorschau zu öffnen.</p></div>';
    }
  } catch (error) {
    showError(error instanceof Error ? error.message : "Katalog konnte nicht geladen werden.");
  }
}

async function selectRoot(root: LibraryRoot, replaceUrl = false): Promise<void> {
  selectedRoot = root;
  location = { ...location, root: root.key, directory: "", offset: 0, file: "" };
  setText("#root-title", root.name);
  viewer?.dispose();
  viewer = undefined;
  detailsElement.innerHTML = '<div class="detail-empty"><span class="forge-rune">◇</span><h2>Bauteil auswählen</h2><p>Wähle eine CAD-Datei, um Metadaten und 3D-Vorschau zu öffnen.</p></div>';
  await revealDirectory(root.key, "");
  await loadCatalog(replaceUrl);
}

async function openFolder(path: string): Promise<void> {
  location = { ...location, directory: path, offset: 0, file: "" };
  if (selectedRoot) await revealDirectory(selectedRoot.key, path);
  rawShell.classList.remove("tree-open");
  await loadCatalog();
}

function renderDetails(file: CadFile, previews: Preview[]): void {
  viewer?.dispose();
  viewer = undefined;
  const preview = newestReadyPreview(previews);
  const status = previews[0]?.status ?? "none";
  detailsElement.innerHTML = `
    <div class="viewer-wrap"><canvas id="viewer"></canvas><div id="viewer-state" class="viewer-state"></div><div class="viewer-hint">Ziehen · Drehen &nbsp; Scrollen · Zoomen</div></div>
    <div class="detail-content"><p class="eyebrow">AUSGEWÄHLTES OBJEKT</p><h2 id="detail-name"></h2><p id="detail-path" class="detail-path"></p>
      <div class="status-line"><span class="status-dot ${status}"></span><span>${status === "none" ? "Keine Vorschau vorhanden" : previewLabel(status)}</span></div>
      <dl><div><dt>Format</dt><dd>${file.format?.toUpperCase() ?? "CAD"}</dd></div><div><dt>Dateigröße</dt><dd>${formatBytes(file.byteSize)}</dd></div><div><dt>Geändert</dt><dd>${formatDate(file.modifiedAtUnixMs)}</dd></div><div><dt>SHA-256</dt><dd class="hash" title="${file.sha256}">${file.sha256.slice(0, 16)}…</dd></div></dl>
      <button id="move-file" class="managed-action">↳ Datei verschieben</button>
    </div>`;
  setText("#detail-name", fileName(file.path));
  setText("#detail-path", parentPath(file.path));
  detailsElement.querySelector<HTMLButtonElement>("#move-file")!.addEventListener("click", () => void openMoveDialog(file));
  const state = document.querySelector<HTMLElement>("#viewer-state")!;
  const artifact = preview?.artifacts.find((item) => item.kind === "preview-glb");
  if (!artifact) {
    state.textContent = status === "none" ? "Noch keine 3D-Vorschau" : previewLabel(status);
    state.classList.add("visible");
    return;
  }
  state.textContent = `3D-Modell wird geladen · ${formatBytes(artifact.byteSize)}`;
  state.classList.add("visible");
  viewer = new CadViewer(document.querySelector<HTMLCanvasElement>("#viewer")!);
  void viewer.load(artifact.url).then(
    () => state.classList.remove("visible"),
    () => {
      state.textContent = "3D-Vorschau konnte nicht geladen werden";
      state.classList.add("visible", "error");
    },
  );
}

async function openMoveDialog(file: CadFile): Promise<void> {
  moveSource = file;
  moveDirectory = parentDirectory(file.path);
  host.querySelector<HTMLElement>("#move-source")!.textContent = file.path;
  host.querySelector<HTMLElement>("#move-error")!.textContent = "";
  moveDialog.showModal();
  await loadMoveFolders();
}

async function loadMoveFolders(): Promise<void> {
  if (!selectedRoot || !moveSource) return;
  moveFoldersElement.innerHTML = '<div class="move-loading">Unterordner werden geladen …</div>';
  renderMoveDestination();
  try {
    const loaded = await catalogApi.folders(selectedRoot.key, {
      directory: moveDirectory, query: "", format: "", sort: "path", direction: "asc", offset: 0, limit: 200,
    });
    moveFoldersElement.replaceChildren();
    for (const folder of loaded.items) {
      const button = document.createElement("button");
      button.innerHTML = `<span>▱</span><span><strong></strong><small></small></span><b>Öffnen →</b>`;
      button.querySelector("strong")!.textContent = folder.name;
      button.querySelector("small")!.textContent = folderCountLabel(folder.fileCount);
      button.addEventListener("click", () => {
        moveDirectory = folder.path;
        void loadMoveFolders();
      });
      moveFoldersElement.append(button);
    }
    if (loaded.items.length === 0) moveFoldersElement.innerHTML = '<div class="move-loading">Keine weiteren Unterordner</div>';
  } catch (error) {
    host.querySelector<HTMLElement>("#move-error")!.textContent = error instanceof Error ? error.message : "Zielordner konnten nicht geladen werden.";
  }
}

function renderMoveDestination(): void {
  if (!moveSource) return;
  moveBreadcrumbsElement.replaceChildren();
  for (const [index, crumb] of folderCrumbs(moveDirectory).entries()) {
    if (index > 0) moveBreadcrumbsElement.append("/");
    const button = document.createElement("button");
    button.textContent = crumb.name;
    button.className = crumb.path === moveDirectory ? "current" : "";
    button.addEventListener("click", () => {
      moveDirectory = crumb.path;
      void loadMoveFolders();
    });
    moveBreadcrumbsElement.append(button);
  }
  const target = moveTargetPath(moveDirectory, moveSource.path);
  host.querySelector<HTMLElement>("#move-target")!.textContent = `Ziel: ${target}`;
  moveConfirmButton.disabled = target === moveSource.path;
}

async function confirmMove(): Promise<void> {
  if (!moveSource || !selectedRoot) return;
  moveConfirmButton.disabled = true;
  moveConfirmButton.textContent = "Wird verschoben …";
  host.querySelector<HTMLElement>("#move-error")!.textContent = "";
  try {
    const moved = await catalogApi.moveFile(moveSource.id, moveDirectory);
    location = { ...location, directory: moveDirectory, file: moved.id, offset: 0, query: "" };
    searchElement.value = "";
    treeChildren.clear();
    treeExpanded.clear();
    roots = await catalogApi.roots();
    selectedRoot = roots.find((root) => root.key === selectedRoot?.key);
    moveDialog.close();
    await revealDirectory(location.root, moveDirectory);
    await loadCatalog(true);
  } catch (error) {
    host.querySelector<HTMLElement>("#move-error")!.textContent = error instanceof Error ? error.message : "Datei konnte nicht verschoben werden.";
  } finally {
    moveConfirmButton.textContent = "Hierher verschieben";
    renderMoveDestination();
  }
}

async function selectFile(file: CadFile, replaceUrl = false): Promise<void> {
  location = { ...location, file: file.id };
  syncUrl(replaceUrl);
  renderEntries();
  detailsElement.innerHTML = '<div class="detail-empty"><span class="forge-rune">◇</span><p>Vorschau wird gesucht …</p></div>';
  try {
    const previews = await catalogApi.previews(file.id);
    if (location.file === file.id) renderDetails(file, previews.items);
  } catch (error) {
    detailsElement.innerHTML = `<div class="detail-empty error"><p></p></div>`;
    detailsElement.querySelector("p")!.textContent = error instanceof Error ? error.message : "Vorschau konnte nicht geladen werden.";
  }
}

async function start(): Promise<void> {
  searchElement.value = location.query;
  formatElement.value = location.format.toUpperCase();
  sortElement.value = `${location.sort}:${location.direction}`;
  try {
    const [health, loadedRoots] = await Promise.all([catalogApi.health(), catalogApi.roots()]);
    setText("#health-text", `System bereit · v${health.version}`);
    document.querySelector("#health-dot")?.classList.add("online");
    roots = loadedRoots;
    setText("#root-count", String(roots.length));
    const initial = roots.find((root) => root.key === location.root) ?? roots[0];
    if (initial) {
      selectedRoot = initial;
      location.root = initial.key;
      setText("#root-title", initial.name);
      await revealDirectory(initial.key, location.directory);
      await loadCatalog(true);
    } else showError("Noch keine CAD-Bibliothek registriert.");
  } catch (error) {
    setText("#health-text", "System nicht erreichbar");
    showError(error instanceof Error ? error.message : "VÖLUND ist nicht erreichbar.");
  }
}

searchElement.addEventListener("input", () => {
  window.clearTimeout(searchTimer);
  searchTimer = window.setTimeout(() => {
    location = { ...location, query: searchElement.value.trim(), offset: 0, file: "" };
    void loadCatalog(true);
  }, 300);
});
formatElement.addEventListener("change", () => {
  location = { ...location, format: formatElement.value.toLowerCase(), offset: 0, file: "" };
  void loadCatalog();
});
sortElement.addEventListener("change", () => {
  const [sort, direction] = sortElement.value.split(":") as [CatalogLocation["sort"], CatalogLocation["direction"]];
  location = { ...location, sort, direction, offset: 0 };
  void loadCatalog();
});
previousButton.addEventListener("click", () => {
  location.offset = Math.max(0, page.offset - PAGE_SIZE);
  void loadCatalog();
});
nextButton.addEventListener("click", () => {
  location.offset = page.offset + PAGE_SIZE;
  void loadCatalog();
});
treeToggle.addEventListener("click", () => rawShell.classList.toggle("tree-open"));
host.querySelector("#move-close")!.addEventListener("click", () => moveDialog.close());
host.querySelector("#move-cancel")!.addEventListener("click", () => moveDialog.close());
moveConfirmButton.addEventListener("click", () => void confirmMove());
const handlePopstate = (): void => {
  location = readCatalogLocation(window.location.search, PAGE_SIZE);
  searchElement.value = location.query;
  formatElement.value = location.format.toUpperCase();
  sortElement.value = `${location.sort}:${location.direction}`;
  const root = roots.find((item) => item.key === location.root) ?? roots[0];
  if (root) {
    selectedRoot = root;
    setText("#root-title", root.name);
    void revealDirectory(root.key, location.directory).then(() => loadCatalog(true));
  }
};
const handleBeforeUnload = (): void => viewer?.dispose();
window.addEventListener("popstate", handlePopstate);
window.addEventListener("beforeunload", handleBeforeUnload);
void start();
return () => {
  window.clearTimeout(searchTimer);
  window.removeEventListener("popstate", handlePopstate);
  window.removeEventListener("beforeunload", handleBeforeUnload);
  viewer?.dispose();
};
}
