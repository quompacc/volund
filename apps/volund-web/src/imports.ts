import { catalogApi, identityApi } from "./api";
import { formatBytes } from "./format";
import { createImportConflictDialog } from "./import-conflict-dialog";
import { importErrorMessage } from "./import-errors";
import { appendTags, buildMetadataRequest, suggestedTags } from "./import-metadata";
import { renderImportReview, showUploadError, updateUploadProgress } from "./import-review-renderer";
import { loadDraftInventory } from "./import-drafts";
import { extensionTarget, importSourceName, importStageLabel, reviewActionLabel, reviewTotalFiles, selectedFilePath, selectedManifest, uploadPercentage } from "./import-view-helpers";
import { settingString } from "./preferences";
import { matchesResumeSelection, importUploadItems } from "./import-resume";
import type { CollectionSummary, ImportDraft, ImportDraftLifecycle, ImportManifestEntry, LibraryRoot, ModelSummary } from "./types";
export { extensionTarget, importSourceName, importStageLabel, reviewActionLabel, reviewTotalFiles, selectedManifest, uploadPercentage } from "./import-view-helpers";
interface PendingModelExtension {
  model: ModelSummary;
  files: File[];
}
let pendingModelExtension: PendingModelExtension | null = null;
let pendingResumeDraft: ImportDraftLifecycle | null = null;
export function queueModelExtension(model: ModelSummary, files: ArrayLike<File>): void {
  pendingModelExtension = { model, files: Array.from(files) };
}

export function mountImportHistory(host: HTMLElement, openModel: (modelId: string) => void, openImports: () => void): void {
  host.innerHTML = `<div class="page-scroll"><header class="page-hero"><div><p class="eyebrow">NACHWEIS</p><h1>Importhistorie</h1><p>Gespeicherte, abgelaufene und abgeschlossene Importvorgänge – getrennt vom aktiven Arbeitsbereich.</p></div></header><section id="import-draft-inventory" class="content-section" aria-live="polite"><p>Importhistorie wird geladen …</p></section></div>`;
  const inventory = host.querySelector<HTMLElement>("#import-draft-inventory")!;
  void loadDraftInventory(inventory, async (draft) => {
    pendingResumeDraft = draft;
    openImports();
  }, openModel);
}

export function mountImports(host: HTMLElement, openModel: (modelId: string) => void): void {
  const queuedExtension = pendingModelExtension;
  pendingModelExtension = null;
  const queuedResumeDraft = pendingResumeDraft;
  pendingResumeDraft = null;
  host.innerHTML = `<div class="page-scroll import-page"><header class="page-hero"><div><p class="eyebrow">EINGANGSSCHLEUSE</p><h1>Modell importieren</h1><p>Ein vollständiges Projektpaket analysieren, bevor Originaldateien in die Bibliothek gelangen.</p></div><span class="prototype-badge live-import-badge">PERSISTENTE IMPORTVORSCHAU</span></header>
    <section class="import-layout"><div id="drop-forge" class="drop-forge"><span class="forge-rune">◇</span><h2>Projektdateien ablegen</h2><p>CAD, STL, 3MF, Bilder, PDF, Tabellen und Dokumentation werden gemeinsam klassifiziert.</p><div class="import-pickers"><label class="primary-action">Dateien auswählen<input id="file-picker" type="file" multiple></label><label class="secondary-action">Ordner auswählen<input id="folder-picker" type="file" multiple webkitdirectory></label></div><small>Die Auswahl wird zunächst analysiert. Originaldateien bleiben unverändert.</small><details class="resume-menu"><summary>Weitere Optionen</summary><button id="resume-import" type="button">Unterbrochenen Upload fortsetzen</button><small>Nur nötig, wenn ein vollständig hochgeladener Entwurf noch nicht importiert wurde.</small></details></div>
      <aside class="import-options"><p class="eyebrow">IMPORTZIEL</p><label class="choice active"><input type="radio" checked name="target" value="create"><span><strong>Neues Modell erstellen</strong><small>Name, Primär-CAD und Ablage automatisch vorschlagen</small></span></label><label class="choice"><input type="radio" name="target" value="extend"><span><strong>Vorhandenes Modell ergänzen</strong><small>Dateien ergänzen; bestehende Modellangaben bleiben erhalten</small></span></label><label id="import-existing-model-label" hidden>Zu ergänzendes Modell<select id="import-existing-model" required><option value="">Modelle werden geladen …</option></select></label><section class="import-library-card"><p class="eyebrow">UPLOAD-ZIEL</p><label>Zielbibliothek<select id="metadata-library" name="libraryRootId" required><option value="">Bibliotheken werden geladen …</option></select></label><small>Neue Dateien werden direkt unter Bauteile, Baugruppen oder Projekte abgelegt – ohne automatisch übernommenen Zwischenordner.</small></section><div class="import-limits"><span>Max. Manifestgröße</span><strong>10 GB · 10.000 Dateien</strong><span>Persistenz</span><strong>PostgreSQL-Entwurf</strong></div></aside></section>
    <section id="import-preview" class="content-section import-preview" hidden><div class="section-heading"><div><p class="eyebrow">ABLAGEVORSCHLAG</p><h2 id="import-model-name">Analyse</h2></div><div class="section-tools"><span id="import-draft-id"></span></div></div><div id="import-summary" class="import-summary"></div><div class="import-review-layout"><form id="import-metadata" class="import-metadata import-metadata-full" hidden><div><p class="eyebrow">MODELLDATEN</p><h3>Import beschreiben</h3><small>Diese Angaben erscheinen später im Dashboard und bleiben unabhängig vom physischen Ablagepfad.</small></div><label class="import-field-name">Modellname<input id="metadata-name" name="modelName" maxlength="160" required></label><label class="import-field-kind">Typ<select id="metadata-kind" name="kind"><option value="assembly">Baugruppe</option><option value="project">Projekt</option><option value="part">Einzelteil</option></select></label><label class="import-field-author">Autor / Quelle<input id="metadata-author" name="authorName" maxlength="160" placeholder="z. B. Voron Design"></label><label class="import-field-tags">Tags<div class="tag-editor-shell"><div id="metadata-tag-editor" class="tag-editor"><div id="metadata-tag-list" class="tag-list"></div><input id="metadata-tags" name="tags" maxlength="50" autocomplete="off" role="combobox" aria-autocomplete="list" aria-expanded="false" aria-controls="metadata-tag-suggestions" placeholder="Vorhandenen Tag suchen oder neu eingeben"></div><div id="metadata-tag-suggestions" class="tag-suggestions" role="listbox" hidden></div></div><small>Vorschlag auswählen oder mit Enter einen neuen Tag erstellen.</small></label><label class="import-field-description">Beschreibung<textarea id="metadata-description" name="description" maxlength="4000" rows="4" placeholder="Kurze Beschreibung des Modells oder Projekts"></textarea></label><p id="metadata-error" class="form-message error" hidden></p><button id="metadata-next" class="primary-action" type="submit">Dateien hochladen & prüfen →</button></form><details class="file-details file-details-full"><summary id="manifest-details-label">Details anzeigen</summary><div id="import-items" class="import-items"></div></details></div></section>
    <section id="import-confirmation" class="content-section import-confirmation" hidden><div class="section-heading"><div><p class="eyebrow">FORTSCHRITT</p><h2 id="upload-phase-title">Wird hochgeladen …</h2></div><span class="ready-mark">SICHERER EINGANGSSPEICHER</span></div><div id="upload-progress" class="upload-progress"><div><span id="upload-progress-label">Upload wird vorbereitet</span><strong id="upload-progress-count">0 / 0</strong></div><div class="upload-progress-track"><i id="upload-progress-bar"></i></div><small id="upload-current-file"></small></div><button id="retry-upload" class="secondary-action retry-action" type="button" hidden>Upload wiederholen</button><p class="transfer-note">Die Übertragung schreibt ausschließlich in den isolierten Eingangsspeicher. Die Bibliothek bleibt bis zum Klick auf „Importieren“ unverändert.</p></section>
    <section id="import-review" class="content-section final-review" hidden><div class="section-heading"><div><p class="eyebrow">ZUSAMMENFASSUNG</p><h2 id="review-title">Importplan</h2></div><span id="review-state" class="ready-mark"></span></div><div id="review-summary" class="import-summary review-summary"></div><div class="review-destination"><span>Zielstruktur</span><strong id="review-destination"></strong><small id="review-model-action"></small></div><div class="review-confirmation"><div><strong id="review-confirm-title">Bereit zum Import</strong><small id="review-confirm-note">Sicherheitsprüfungen, Konfliktkontrolle und Rollback laufen automatisch.</small></div><div class="review-actions"><button id="commit-import" class="primary-action" type="button" disabled>Importieren</button><button id="open-model" class="secondary-action" type="button" hidden>Modell öffnen</button></div></div><p id="review-error" class="form-message error" hidden></p><details class="file-details review-details"><summary id="review-details-label">Details anzeigen</summary><div id="review-items" class="review-items"></div></details></section>
    <section class="content-section"><div class="section-heading"><div><p class="eyebrow">EINFACHER ABLAUF</p><h2>Vom Paket zum Modell</h2></div></div><div class="pipeline"><article><b>01</b><strong>Auswählen</strong><p>Ordner oder einzelne Projektdateien ablegen.</p></article><article><b>02</b><strong>Beschreiben</strong><p>Name, Typ, Autor und Tags ergänzen.</p></article><article><b>03</b><strong>Hochladen & prüfen</strong><p>Fortschritt und kompakte Zusammenfassung ansehen.</p></article><article><b>04</b><strong>Importieren</strong><p>Ein Klick übernimmt das geprüfte Modell atomar.</p></article></div></section><dialog id="import-selection-dialog" class="import-selection-dialog"><form method="dialog"><p class="eyebrow">UPLOAD VORBEREITEN</p><h2>Auswahl übernehmen?</h2><div class="selection-summary"><strong id="selection-file-count">0 Dateien</strong><span id="selection-total-size">0 B</span></div><p id="selection-source-name"></p><small>Erst nach deiner Bestätigung analysiert VÖLUND die Auswahl. In die Bibliothek wird noch nichts geschrieben.</small><div class="dialog-actions"><button id="cancel-selection" class="secondary-action" value="cancel" type="button">Abbrechen</button><button id="confirm-selection" class="primary-action" value="confirm" type="button">Auswahl analysieren →</button></div></form></dialog></div>`;
  const conflictDialog = createImportConflictDialog();
  host.append(conflictDialog.element);

  const collectionField = document.createElement("fieldset");
  collectionField.className = "import-collection-field";
  collectionField.innerHTML = `<legend>Kollektionen</legend><div id="metadata-collection-list" class="collection-options"><small>Kollektionen werden geladen …</small></div><div class="new-collection"><input id="new-collection-name" maxlength="160" placeholder="Neue Kollektion"><button id="create-collection" class="secondary-action" type="button">Erstellen</button></div>`;
  host.querySelector<HTMLInputElement>("#metadata-author")!.closest("label")!.after(collectionField);
  const authorOptions=document.createElement("datalist"); authorOptions.id="metadata-author-options";
  host.querySelector<HTMLInputElement>("#metadata-author")!.setAttribute("list",authorOptions.id);
  host.append(authorOptions);
  const targetField = document.createElement("fieldset");
  targetField.className = "import-target-field";
  targetField.innerHTML = `<legend>Publikationsziel</legend><label>Entscheidung<select id="metadata-target-action"><option value="create">Neues Modell erstellen</option><option value="extend">Nur Dateien sicher ergänzen</option><option value="update">Modell und Metadaten aktualisieren</option></select></label><label id="metadata-target-model-label" hidden>Zielmodell<select id="metadata-target-model"></select></label><p id="metadata-extension-note" class="form-message" hidden>Bestehende Metadaten, Tags, Kollektionen, Primärquelle und Dateiverknüpfungen bleiben unverändert.</p></fieldset>`;
  host.querySelector<HTMLSelectElement>("#metadata-kind")!.closest("label")!.after(targetField);
  const sourceField = document.createElement("fieldset");
  sourceField.className = "import-source-field";
  sourceField.innerHTML = `<legend>Dateirollen</legend><label>Primärquelle<select id="metadata-primary"><option value="">Automatischer Vorschlag</option></select></label><label>Vorschaubild<select id="metadata-thumbnail"><option value="">Neutraler Platzhalter</option></select><small>Wähle eine echte Bilddatei aus dem Import. Ohne Auswahl bleibt das Modellbild neutral.</small></label><label>Lizenz<select id="metadata-license-kind"><option value="not-specified">Nicht angegeben</option><option value="spdx">SPDX</option><option value="custom">Benutzerdefiniert</option></select></label><label id="metadata-license-value-label" hidden>Lizenzwert<input id="metadata-license-value" maxlength="160"></label></fieldset>`;
  collectionField.after(sourceField);
  let selectedTags: string[] = [];
  let availableTagNames: string[] = [];
  let selectedCollectionIds = new Set<string>();
  let availableCollections: CollectionSummary[] = [];
  let availableRoots: LibraryRoot[] = [];
  let availableModels: ModelSummary[] = queuedExtension ? [queuedExtension.model] : [];
  let defaultModelKind = "assembly";
  let selectedFiles: File[] = [];
  let pendingSelection: File[] = [];
  let currentDraft: ImportDraft | null = null;
  let resumeDraft: ImportDraft | null = null;
  const tagInput = host.querySelector<HTMLInputElement>("#metadata-tags")!;
  const tagList = host.querySelector<HTMLElement>("#metadata-tag-list")!;
  const tagSuggestionList = host.querySelector<HTMLElement>("#metadata-tag-suggestions")!;
  const collectionList = host.querySelector<HTMLElement>("#metadata-collection-list")!;
  const librarySelect = host.querySelector<HTMLSelectElement>("#metadata-library")!;
  const renderRoots = (): void => {
    librarySelect.replaceChildren(...availableRoots.map((root) => {
      const option = document.createElement("option");
      option.value = root.id;
      option.textContent = `${root.name} · ${root.key}`;
      return option;
    }));
    if (availableRoots.length === 0) {
      const option = document.createElement("option");
      option.value = "";
      option.textContent = "Keine Bibliothek registriert";
      librarySelect.append(option);
    }
  };
  const renderModels = (): void => {
    const select = host.querySelector<HTMLSelectElement>("#metadata-target-model")!;
    const options = availableModels.map((model) => {
      const option=document.createElement("option"); option.value=model.id;
      option.dataset.revision=String(model.revision); option.textContent=`${model.name} · Revision ${model.revision}`;
      return option;
    });
    select.replaceChildren(...options.map((option) => option.cloneNode(true) as HTMLOptionElement));
    const earlySelect = host.querySelector<HTMLSelectElement>("#import-existing-model")!;
    earlySelect.replaceChildren(...options);
    if (availableModels.length === 0) {
      const empty = new Option("Keine Modelle vorhanden", "");
      select.append(empty.cloneNode(true) as HTMLOptionElement);
      earlySelect.append(empty);
    }
  };

  const syncImportTarget = (): void => {
    const selected = host.querySelector<HTMLInputElement>('input[name="target"]:checked')?.value ?? "create";
    const extending = selected === "extend";
    host.querySelectorAll<HTMLElement>(".import-options .choice").forEach((choice) => {
      choice.classList.toggle("active", (choice.querySelector("input") as HTMLInputElement).checked);
    });
    host.querySelector<HTMLElement>("#import-existing-model-label")!.hidden = !extending;
    const earlySelect = host.querySelector<HTMLSelectElement>("#import-existing-model")!;
    earlySelect.disabled = !extending;
    const action = host.querySelector<HTMLSelectElement>("#metadata-target-action")!;
    const model = host.querySelector<HTMLSelectElement>("#metadata-target-model")!;
    action.value = extending ? "extend" : "create";
    model.value = extending ? earlySelect.value : "";
    host.querySelector<HTMLElement>("#metadata-target-model-label")!.hidden = !extending;
    host.querySelector<HTMLElement>("#metadata-extension-note")!.hidden = !extending;
    const primary = host.querySelector<HTMLSelectElement>("#metadata-primary")!;
    primary.disabled = extending;
    if (extending) primary.value = "";
  };
  const renderCollections = (): void => {
    if (availableCollections.length === 0) {
      collectionList.innerHTML = "<small>Noch keine Kollektion vorhanden.</small>";
      return;
    }
    collectionList.replaceChildren(...availableCollections.map((collection) => {
      const label = document.createElement("label");
      const input = document.createElement("input");
      input.type = "checkbox";
      input.value = collection.id;
      input.checked = selectedCollectionIds.has(collection.id);
      label.append(input, collection.name);
      return label;
    }));
  };
  const renderTags = (): void => {
    tagList.replaceChildren(...selectedTags.map((tag) => {
      const chip = document.createElement("span");
      chip.className = "metadata-tag-chip";
      chip.dataset.tag = tag;
      chip.append(tag);
      const remove = document.createElement("button");
      remove.type = "button";
      remove.dataset.removeTag = tag;
      remove.setAttribute("aria-label", `Tag ${tag} entfernen`);
      remove.textContent = "×";
      chip.append(remove);
      return chip;
    }));
  };
  const renderTagSuggestions = (): void => {
    const matches = suggestedTags(availableTagNames, selectedTags, tagInput.value);
    tagSuggestionList.replaceChildren(...matches.map((tag) => {
      const option = document.createElement("button");
      option.type = "button";
      option.role = "option";
      option.dataset.suggestedTag = tag;
      option.textContent = tag;
      return option;
    }));
    tagSuggestionList.hidden = matches.length === 0;
    tagInput.setAttribute("aria-expanded", String(matches.length > 0));
  };
  const acceptTags = (): void => {
    selectedTags = appendTags(selectedTags, tagInput.value);
    tagInput.value = "";
    renderTags();
    renderTagSuggestions();
  };
  const applyQueuedExtension = (): void => {
    if (!queuedExtension) return;
    const target = extensionTarget(queuedExtension.model);
    const action = host.querySelector<HTMLSelectElement>("#metadata-target-action")!;
    const modelSelect = host.querySelector<HTMLSelectElement>("#metadata-target-model")!;
    const earlyModelSelect = host.querySelector<HTMLSelectElement>("#import-existing-model")!;
    const extendRadio = host.querySelector<HTMLInputElement>('input[name="target"][value="extend"]')!;
    extendRadio.checked = true;
    earlyModelSelect.value = target.modelId;
    action.value = target.action;
    modelSelect.value = target.modelId;
    action.disabled = true;
    modelSelect.disabled = true;
    host.querySelector<HTMLElement>("#metadata-target-model-label")!.hidden = false;
    host.querySelector<HTMLElement>("#metadata-extension-note")!.hidden = false;
    host.querySelector<HTMLInputElement>("#metadata-name")!.value = queuedExtension.model.name;
    host.querySelector<HTMLSelectElement>("#metadata-kind")!.value = queuedExtension.model.kind;
    const primary = host.querySelector<HTMLSelectElement>("#metadata-primary")!;
    primary.value = "";
    primary.disabled = true;
  };
  const selectionDialog = host.querySelector<HTMLDialogElement>("#import-selection-dialog")!;
  const closeSelectionDialog = (): void => {
    if (selectionDialog.open && typeof selectionDialog.close === "function") selectionDialog.close();
    else selectionDialog.removeAttribute("open");
  };
  const presentSelection = (files: ArrayLike<File>): void => {
    const entries = selectedManifest(files);
    if (entries.length === 0) return;
    pendingSelection = Array.from(files);
    host.querySelector<HTMLElement>("#selection-file-count")!.textContent = `${entries.length} Dateien`;
    host.querySelector<HTMLElement>("#selection-total-size")!.textContent = formatBytes(entries.reduce((sum, entry) => sum + entry.byteSize, 0));
    host.querySelector<HTMLElement>("#selection-source-name")!.textContent = importSourceName(entries);
    if (typeof selectionDialog.showModal === "function") selectionDialog.showModal();
    else selectionDialog.setAttribute("open", "");
  };
  const analyze = async (files: ArrayLike<File>): Promise<void> => {
    const entries = selectedManifest(files);
    if (entries.length === 0) return;
    showImportLoading(host, entries);
    try {
      selectedTags = [];
      selectedCollectionIds = new Set();
      selectedFiles = Array.from(files);
      renderTags();
      renderCollections();
      if (resumeDraft) {
        if (!matchesResumeSelection(resumeDraft.items, entries))
          throw new Error("Wähle die ausstehenden Dateien und ursprünglichen ZIP-Archive mit unveränderten Namen und Größen aus.");
        currentDraft = resumeDraft;
        resumeDraft = null;
      } else currentDraft = await catalogApi.previewImport(importSourceName(entries), entries);
      renderImportDraft(host, currentDraft);
      host.querySelector<HTMLSelectElement>("#metadata-kind")!.value = defaultModelKind;
      applyQueuedExtension();
      syncImportTarget();
    } catch (error) {
      showImportError(host, error instanceof Error ? error.message : "Import konnte nicht analysiert werden.");
    }
  };
  host.querySelector<HTMLInputElement>("#file-picker")!.addEventListener("change", (event) => {
    presentSelection((event.currentTarget as HTMLInputElement).files!);
  });
  host.querySelector<HTMLInputElement>("#folder-picker")!.addEventListener("change", (event) => {
    presentSelection((event.currentTarget as HTMLInputElement).files!);
  });
  host.querySelector<HTMLButtonElement>("#confirm-selection")!.addEventListener("click", () => {
    const files = pendingSelection;
    pendingSelection = [];
    closeSelectionDialog();
    void analyze(files);
  });
  host.querySelector<HTMLButtonElement>("#cancel-selection")!.addEventListener("click", () => {
    pendingSelection = [];
    host.querySelector<HTMLInputElement>("#file-picker")!.value = "";
    host.querySelector<HTMLInputElement>("#folder-picker")!.value = "";
    closeSelectionDialog();
  });
  const drop = host.querySelector<HTMLElement>("#drop-forge")!;
  drop.addEventListener("dragover", (event) => {
    event.preventDefault();
    drop.classList.add("dragging");
  });
  drop.addEventListener("dragleave", () => drop.classList.remove("dragging"));
  drop.addEventListener("drop", (event) => {
    event.preventDefault();
    drop.classList.remove("dragging");
    if (event.dataTransfer?.files) presentSelection(event.dataTransfer.files);
  });
  host.querySelector<HTMLFormElement>("#import-metadata")!.addEventListener("submit", (event) => {
    event.preventDefault();
    acceptTags();
    if (currentDraft) void saveMetadata(host, selectedTags, [...selectedCollectionIds], currentDraft, selectedFiles, availableModels);
  });
  tagInput.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" && event.key !== ",") return;
    event.preventDefault();
    acceptTags();
  });
  tagInput.addEventListener("input", renderTagSuggestions);
  tagInput.addEventListener("focus", renderTagSuggestions);
  tagInput.addEventListener("blur", () => window.setTimeout(() => {
    tagSuggestionList.hidden = true;
    tagInput.setAttribute("aria-expanded", "false");
  }, 120));
  tagSuggestionList.addEventListener("mousedown", (event) => {
    event.preventDefault();
    const tag = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-suggested-tag]")?.dataset.suggestedTag;
    if (!tag) return;
    selectedTags = appendTags(selectedTags, tag);
    tagInput.value = "";
    renderTags();
    renderTagSuggestions();
    tagInput.focus();
  });
  tagList.addEventListener("click", (event) => {
    const tag = (event.target as HTMLElement).dataset.removeTag;
    if (!tag) return;
    selectedTags = selectedTags.filter((candidate) => candidate !== tag);
    renderTags();
    tagInput.focus();
  });
  collectionList.addEventListener("change", (event) => {
    const input = event.target as HTMLInputElement;
    if (input.checked) selectedCollectionIds.add(input.value);
    else selectedCollectionIds.delete(input.value);
  });
  host.querySelector<HTMLButtonElement>("#create-collection")!.addEventListener("click", async () => {
    const input = host.querySelector<HTMLInputElement>("#new-collection-name")!;
    const button = host.querySelector<HTMLButtonElement>("#create-collection")!;
    const error = host.querySelector<HTMLElement>("#metadata-error")!;
    if (!input.value.trim()) return input.focus();
    button.disabled = true;
    try {
      const collection = await catalogApi.createCollection({ name: input.value, description: "" });
      availableCollections.push(collection);
      selectedCollectionIds.add(collection.id);
      input.value = "";
      error.hidden = true;
      renderCollections();
    } catch (reason) {
      error.textContent = reason instanceof Error ? reason.message : "Kollektion konnte nicht erstellt werden.";
      error.hidden = false;
    } finally {
      button.disabled = false;
    }
  });
  host.querySelector<HTMLButtonElement>("#retry-upload")!.addEventListener("click", () => {
    if (currentDraft) void uploadDraft(host, currentDraft, selectedFiles);
  });
  host.querySelector<HTMLButtonElement>("#resume-import")!.addEventListener("click", () => {
    void resumeLatestImport(host);
  });
  host.querySelector<HTMLButtonElement>("#commit-import")!.addEventListener("click", () => {
    const draftId = host.querySelector<HTMLElement>("#import-review")!.dataset.draftId;
    if (draftId) void confirmImport(host, draftId);
  });
  host.querySelector<HTMLElement>("#review-items")!.addEventListener("click", async (event) => {
    const button=(event.target as HTMLElement).closest<HTMLButtonElement>("button[data-resolution]");
    const draftId=host.querySelector<HTMLElement>("#import-review")!.dataset.draftId;
    if(!button||!draftId)return;
    const target=button.dataset.resolution==="create" ? await conflictDialog.requestPath(button.dataset.targetPath ?? "") : null;
    if(button.dataset.resolution==="create"&&!target)return;
    try { await catalogApi.resolveImportItem(draftId,button.dataset.itemId!,button.dataset.resolution as "create"|"skip",target); await showImportReview(host,draftId); }
    catch(reason) { showImportError(host,reason instanceof Error?reason.message:"Konflikt konnte nicht gelöst werden."); }
  });
  host.querySelector<HTMLButtonElement>("#open-model")!.addEventListener("click", (event) => {
    const modelId = (event.currentTarget as HTMLButtonElement).dataset.modelId;
    if (modelId) openModel(modelId);
  });
  host.querySelector<HTMLSelectElement>("#metadata-target-action")!.addEventListener("change", (event) => {
    const value = (event.currentTarget as HTMLSelectElement).value;
    host.querySelector<HTMLElement>("#metadata-target-model-label")!.hidden = value === "create";
    host.querySelector<HTMLElement>("#metadata-extension-note")!.hidden = value !== "extend";
    const primary = host.querySelector<HTMLSelectElement>("#metadata-primary")!;
    primary.disabled = value === "extend";
    if (primary.disabled) primary.value = "";
  });
  host.querySelectorAll<HTMLInputElement>('input[name="target"]').forEach((radio) => radio.addEventListener("change", syncImportTarget));
  host.querySelector<HTMLSelectElement>("#import-existing-model")!.addEventListener("change", syncImportTarget);
  host.querySelector<HTMLSelectElement>("#metadata-license-kind")!.addEventListener("change", (event) => {
    host.querySelector<HTMLElement>("#metadata-license-value-label")!.hidden =
      (event.currentTarget as HTMLSelectElement).value === "not-specified";
  });
  void catalogApi.collections().then((collections) => {
    availableCollections = collections;
    renderCollections();
  }, () => {
    collectionList.innerHTML = "<small>Kollektionen konnten nicht geladen werden.</small>";
  });
  void catalogApi.roots().then((roots) => {
    availableRoots = roots;
    renderRoots();
  }, () => {
    availableRoots = [];
    renderRoots();
  });
  void catalogApi.models().then((models) => {
    availableModels = queuedExtension && !models.some((model) => model.id === queuedExtension.model.id)
      ? [queuedExtension.model, ...models] : models;
    renderModels();
    applyQueuedExtension();
    syncImportTarget();
  });
  void catalogApi.authors().then((page)=>authorOptions.replaceChildren(...page.items.filter((item)=>item.active)
    .map((item)=>new Option(item.name,item.name))));
  void catalogApi.tags().then((page) => {
    availableTagNames = page.items.filter((item) => item.active).map((item) => item.name);
    renderTagSuggestions();
  });
  void identityApi.settings().then((settings) => {
    const preferredKind = settingString(settings, "imports.defaultModelKind", "assembly");
    const select = host.querySelector<HTMLSelectElement>("#metadata-kind");
    defaultModelKind = preferredKind;
    if (select) select.value = defaultModelKind;
  }, () => undefined);
  if (queuedExtension?.files.length) {
    renderModels();
    queueMicrotask(() => void analyze(queuedExtension.files));
  }
  if (queuedResumeDraft) {
    if (["uploaded", "review_ready", "reviewed", "failed"].includes(queuedResumeDraft.status)) {
      void showImportReview(host, queuedResumeDraft.id);
    } else {
      void catalogApi.importDraftManifest(queuedResumeDraft.id).then((draft) => {
        resumeDraft = draft;
        showImportError(host, "Bereits hochgeladene Dateien bleiben erhalten. Wähle die ausstehenden Dateien und ursprünglichen ZIP-Archive; entpackte Dateien werden nicht benötigt.");
      });
    }
  }
}

function showImportLoading(host: HTMLElement, entries: ImportManifestEntry[]): void {
  const preview = host.querySelector<HTMLElement>("#import-preview")!;
  preview.hidden = false;
  host.querySelector<HTMLElement>("#import-model-name")!.textContent = importStageLabel("analyzing");
  const summary = host.querySelector<HTMLElement>("#import-summary")!;
  summary.classList.remove("error");
  summary.textContent = `${entries.length} Dateien werden sicher geprüft.`;
  host.querySelector<HTMLElement>("#import-items")!.replaceChildren();
}

function renderImportDraft(host: HTMLElement, draft: ImportDraft): void {
  host.querySelector<HTMLElement>("#import-model-name")!.textContent = draft.suggestedModelName;
  host.querySelector<HTMLElement>("#import-draft-id")!.textContent = `ENTWURF ${draft.id.slice(0, 8)}`;
  const categories = draft.items.reduce<Record<string, number>>((counts, item) => {
    counts[item.category] = (counts[item.category] ?? 0) + 1;
    return counts;
  }, {});
  const summary = host.querySelector<HTMLElement>("#import-summary")!;
  summary.classList.remove("error");
  summary.innerHTML = `<article><strong>${draft.totalFiles}</strong><span>Dateien</span></article><article><strong>${formatBytes(draft.totalBytes)}</strong><span>Gesamtgröße</span></article><article><strong>${draft.suggestedSlug}</strong><span>Modellordner</span></article><article><strong>${Object.keys(categories).length}</strong><span>Kategorien</span></article>`;
  const items = host.querySelector<HTMLElement>("#import-items")!;
  items.replaceChildren(...draft.items.slice(0, 200).map(importItemRow));
  if (draft.items.length > 200) items.append(`${draft.items.length - 200} weitere Dateien werden nach Bestätigung übernommen.`);
  host.querySelector<HTMLDetailsElement>(".import-review-layout .file-details")!.open = false;
  host.querySelector<HTMLElement>("#manifest-details-label")!.textContent = `Details anzeigen · ${draft.totalFiles} Dateien`;
  const form = host.querySelector<HTMLFormElement>("#import-metadata")!;
  form.reset();
  form.hidden = false;
  form.dataset.draftId = draft.id;
  host.querySelector<HTMLInputElement>("#metadata-name")!.value = draft.suggestedModelName;
  const primary=host.querySelector<HTMLSelectElement>("#metadata-primary")!;
  const thumbnail=host.querySelector<HTMLSelectElement>("#metadata-thumbnail")!;
  primary.replaceChildren(new Option("Automatischer Vorschlag", ""), ...draft.items
    .filter((item)=>item.category==="cad"||item.category==="mesh")
    .map((item)=>new Option(item.originalPath,item.id)));
  thumbnail.replaceChildren(new Option("Automatisch nach erfolgreicher Vorschau", ""), ...draft.items
    .filter((item)=>item.category==="image" && !item.originalPath.toLowerCase().endsWith(".svg"))
    .map((item)=>new Option(item.originalPath,item.id)));
  host.querySelector<HTMLElement>("#import-confirmation")!.hidden = true;
  host.querySelector<HTMLElement>("#import-review")!.hidden = true;
  form.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

async function saveMetadata(
  host: HTMLElement,
  tags: string[],
  collectionIds: string[],
  draft: ImportDraft,
  files: File[],
  models: ModelSummary[],
): Promise<void> {
  const form = host.querySelector<HTMLFormElement>("#import-metadata")!;
  const button = host.querySelector<HTMLButtonElement>("#metadata-next")!;
  const error = host.querySelector<HTMLElement>("#metadata-error")!;
  error.hidden = true;
  try {
    const targetAction=host.querySelector<HTMLSelectElement>("#metadata-target-action")!.value as "create"|"update"|"extend";
    const targetId=host.querySelector<HTMLSelectElement>("#metadata-target-model")!.value;
    const request = buildMetadataRequest({
      modelName: host.querySelector<HTMLInputElement>("#metadata-name")!.value,
      kind: host.querySelector<HTMLSelectElement>("#metadata-kind")!.value,
      libraryRootId: host.querySelector<HTMLSelectElement>("#metadata-library")!.value,
      authorName: host.querySelector<HTMLInputElement>("#metadata-author")!.value,
      tags,
      description: host.querySelector<HTMLTextAreaElement>("#metadata-description")!.value,
      collectionIds,
      targetAction,
      targetModel: targetAction!=="create" ? models.find((model)=>model.id===targetId) ?? null : null,
      licenseKind: host.querySelector<HTMLSelectElement>("#metadata-license-kind")!.value as "not-specified"|"spdx"|"custom",
      licenseValue: host.querySelector<HTMLInputElement>("#metadata-license-value")!.value,
      primaryItemId: host.querySelector<HTMLSelectElement>("#metadata-primary")!.value || null,
      thumbnailItemId: host.querySelector<HTMLSelectElement>("#metadata-thumbnail")!.value || null,
    });
    button.disabled = true;
    button.textContent = "Upload wird vorbereitet …";
    await catalogApi.configureImport(form.dataset.draftId!, request);
    await uploadDraft(host, draft, files);
  } catch (reason) {
    error.textContent = importErrorMessage(reason instanceof Error ? reason.message : "Metadaten konnten nicht gespeichert werden.");
    error.hidden = false;
    button.disabled = false;
    button.textContent = "Erneut hochladen & prüfen";
  }
}

async function uploadDraft(host: HTMLElement, draft: ImportDraft, files: File[]): Promise<void> {
  const progress = host.querySelector<HTMLElement>("#upload-progress")!;
  const retry = host.querySelector<HTMLButtonElement>("#retry-upload")!;
  const confirmation = host.querySelector<HTMLElement>("#import-confirmation")!;
  confirmation.hidden = false;
  confirmation.scrollIntoView({ behavior: "smooth", block: "start" });
  progress.hidden = false;
  progress.classList.remove("error", "complete");
  retry.hidden = true;
  host.querySelector<HTMLElement>("#upload-phase-title")!.textContent = importStageLabel("uploading");
  const byPath = new Map(files.map((file) => [selectedFilePath(file), file]));
  const uploadItems = importUploadItems(draft.items);
  const missing = uploadItems.find((item) => !byPath.has(item.originalPath));
  if (missing) {
    showUploadError(host, `Die Browser-Auswahl enthält ${missing.originalPath} nicht mehr. Bitte den Ordner erneut auswählen.`);
    retry.hidden = false;
    return;
  }
  let completed = draft.items.length - uploadItems.length;
  try {
    for (const item of uploadItems) {
      host.querySelector<HTMLElement>("#upload-current-file")!.textContent = item.originalPath;
      host.querySelector<HTMLElement>("#upload-progress-label")!.textContent = importStageLabel("uploading");
      await catalogApi.uploadImportItem(draft.id, item.id, byPath.get(item.originalPath)!);
      completed += 1;
      updateUploadProgress(host, completed, draft.items.length);
    }
    progress.classList.add("complete");
    host.querySelector<HTMLElement>("#upload-progress-label")!.textContent = "Sicher im Eingangsspeicher";
    host.querySelector<HTMLElement>("#upload-current-file")!.textContent = "SHA-256 für alle Dateien gespeichert · Bibliothek noch unverändert";
    host.querySelector<HTMLElement>("#upload-phase-title")!.textContent = importStageLabel("reviewing");
    await showImportReview(host, draft.id);
    host.querySelector<HTMLButtonElement>("#metadata-next")!.textContent = "Upload abgeschlossen";
  } catch (reason) {
    showUploadError(host, reason instanceof Error ? reason.message : "Dateiübertragung fehlgeschlagen.");
    retry.hidden = false;
    host.querySelector<HTMLButtonElement>("#metadata-next")!.textContent = "Upload unterbrochen";
  }
}

async function resumeLatestImport(host: HTMLElement): Promise<void> {
  const button = host.querySelector<HTMLButtonElement>("#resume-import")!;
  button.disabled = true;
  button.textContent = "Upload wird gesucht …";
  try {
    const draft = await catalogApi.latestUploadedImport();
    if (!draft) throw new Error("Es gibt noch keinen vollständig übertragenen Import.");
    await showImportReview(host, draft.id);
  } catch (reason) {
    showImportError(host, reason instanceof Error ? reason.message : "Import konnte nicht geladen werden.");
  } finally {
    button.disabled = false;
    button.textContent = "Unterbrochenen Upload fortsetzen";
  }
}

async function showImportReview(host: HTMLElement, draftId: string): Promise<void> {
  const section = host.querySelector<HTMLElement>("#import-review")!;
  section.hidden = false;
  host.querySelector<HTMLElement>("#review-title")!.textContent = importStageLabel("reviewing");
  host.querySelector<HTMLElement>("#review-summary")!.replaceChildren();
  host.querySelector<HTMLElement>("#review-items")!.replaceChildren();
  const review = await catalogApi.reviewImport(draftId);
  renderImportReview(host, review);
}

async function confirmImport(host: HTMLElement, draftId: string): Promise<void> {
  const button = host.querySelector<HTMLButtonElement>("#commit-import")!;
  const error = host.querySelector<HTMLElement>("#review-error")!;
  button.disabled = true;
  button.textContent = importStageLabel("committing");
  error.hidden = true;
  try {
    const result = await catalogApi.commitImport(draftId);
    host.querySelector<HTMLElement>("#review-state")!.textContent = "✓ ÜBERNOMMEN";
    host.querySelector<HTMLElement>("#review-confirm-title")!.textContent = importStageLabel("complete");
    host.querySelector<HTMLElement>("#review-confirm-note")!.textContent = "Modell, Tags und stabile Dateiverknüpfungen sind jetzt gespeichert.";
    button.textContent = `${result.totalFiles} Dateien übernommen`;
    host.querySelector<HTMLButtonElement>("#open-model")!.hidden = false;
    host.querySelector<HTMLButtonElement>("#open-model")!.dataset.modelId = result.modelId;
    host.querySelector<HTMLElement>("#review-items")!.classList.add("committed");
  } catch (reason) {
    error.textContent = importErrorMessage(reason instanceof Error ? reason.message : "Import konnte nicht bestätigt werden.");
    error.hidden = false;
    button.disabled = false;
    button.textContent = "Import erneut versuchen";
  }
}

function importItemRow(item: ImportDraft["items"][number]): HTMLElement {
  const row = document.createElement("article");
  row.className = item.isPrimaryCandidate ? "primary-candidate" : "";
  row.innerHTML = `<span class="import-category"></span><span><strong></strong><small></small></span><b></b>`;
  row.querySelector(".import-category")!.textContent = item.category.toUpperCase();
  row.querySelector("strong")!.textContent = item.originalPath;
  row.querySelector("small")!.textContent = item.suggestedRelativePath;
  row.querySelector("b")!.textContent = item.isPrimaryCandidate ? "PRIMÄR-CAD" : formatBytes(item.byteSize);
  return row;
}

function showImportError(host: HTMLElement, message: string): void {
  host.querySelector<HTMLElement>("#import-preview")!.hidden = false;
  host.querySelector<HTMLElement>("#import-model-name")!.textContent = "Analyse fehlgeschlagen";
  const summary = host.querySelector<HTMLElement>("#import-summary")!;
  summary.textContent = importErrorMessage(message);
  summary.classList.add("error");
}
