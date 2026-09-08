import { ApiError, libraryApi } from "./api";
import { confirmExact } from "./confirmation";
import type { ManagedLibrary } from "./types";

export function libraryPanel(libraries: ManagedLibrary[]): string {
  return `<section class="admin-section"><div class="section-heading"><div><p class="eyebrow">SPEICHER</p><h2>Bibliotheken</h2></div><span>${libraries.length} registriert</span></div>
    <p class="admin-copy">Absolute Pfade werden auf dem Debian-Host geprüft. Schlüssel und Pfad bleiben nach dem Anlegen unveränderlich.</p>
    <form id="create-library" class="library-form">
      <label>Schlüssel<input name="key" required maxlength="63" pattern="[a-z][a-z0-9_-]{0,62}" placeholder="cad_archiv"></label>
      <label>Anzeigename<input name="name" required maxlength="160" placeholder="CAD-Archiv"></label>
      <label class="library-path">Absoluter Debian-Pfad<input name="path" required maxlength="4096" placeholder="/srv/cad/archive"></label>
      <button type="button" class="secondary-action" data-validate-library>Pfad prüfen</button>
      <button class="primary-action">Bibliothek anlegen</button>
    </form>
    <div id="library-validation" class="library-validation" aria-live="polite"></div>
    <div class="library-list">${libraries.length === 0 ? '<p class="empty-copy">Noch keine Bibliothek registriert.</p>' : libraries.map(libraryRow).join("")}</div>
  </section>`;
}

export function bindLibraryActions(host: HTMLElement, refresh: () => Promise<void>): void {
  const create = host.querySelector<HTMLFormElement>("#create-library");
  create?.querySelector<HTMLButtonElement>("[data-validate-library]")?.addEventListener("click", () => {
    const path = String(new FormData(create).get("path"));
    void run(host, async () => {
      const result = await validateLibraryPath(path);
      const write = result.writablePermission ? "Schreibrechte vorhanden" : "keine Schreibbits gesetzt";
      showValidation(host, `Gültig: ${result.canonicalPath} · lesbar · ${write}`);
    });
  });
  create?.addEventListener("submit", (event) => {
    event.preventDefault();
    const data = new FormData(create);
    void run(host, async () => {
      const path = String(data.get("path"));
      await validateLibraryPath(path);
      try {
        await libraryApi.create({
          key: String(data.get("key")),
          name: String(data.get("name")),
          path,
          confirmation: await confirmExact(`ADD LIBRARY ${String(data.get("key"))}`, "Die Bibliothek erhält einen dauerhaften Schlüssel und Speicherpfad."),
        });
      } catch (error) {
        if (error instanceof ApiError && error.status === 409) throw new Error("Bibliotheksschlüssel oder Pfad ist bereits registriert.");
        throw error;
      }
      await refresh();
    });
  });
  host.querySelectorAll<HTMLFormElement>("[data-library-edit]").forEach((form) => {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      void run(host, async () => {
        try {
          await libraryApi.update(form.dataset.libraryEdit!, {
            expectedRevision: Number(form.dataset.revision),
            name: String(new FormData(form).get("name")),
            confirmation: await confirmExact(`UPDATE LIBRARY ${form.dataset.libraryEdit!}`, "Der Anzeigename dieser Bibliothek wird geändert."),
          });
        } catch (error) {
          if (error instanceof ApiError && error.status === 409) throw new Error("Die Bibliothek wurde zwischenzeitlich geändert. Bitte neu laden und erneut bestätigen.");
          throw error;
        }
        await refresh();
      });
    });
  });
  host.querySelectorAll<HTMLButtonElement>("[data-library-toggle]").forEach((button) => {
    button.addEventListener("click", () => void run(host, async () => {
      const key = button.dataset.libraryToggle!;
      try {
        await libraryApi.update(key, { expectedRevision: Number(button.dataset.revision), enabled: button.dataset.enabled !== "true", confirmation: await confirmExact(`UPDATE LIBRARY ${key}`, "Die Scan-Aktivierung dieser Bibliothek wird geändert; Katalogdaten bleiben erhalten.") });
      } catch (error) {
        if (error instanceof ApiError && error.status === 409) throw new Error("Die Bibliothek wurde zwischenzeitlich geändert. Bitte neu laden und erneut bestätigen.");
        throw error;
      }
      await refresh();
    }));
  });
  host.querySelectorAll<HTMLButtonElement>("[data-library-scan]").forEach((button) => {
    button.addEventListener("click", () => void run(host, async () => {
      const label = button.textContent;
      button.disabled = true;
      button.textContent = "Wird eingereiht …";
      try {
        const key = button.dataset.libraryScan!;
        const full = button.dataset.full === "true";
        const confirmation = full ? await confirmExact(`FULL SCAN ${key}`, "Ein Vollscan liest und hasht die gesamte Bibliothek erneut; Originale bleiben unverändert.") : undefined;
        const result = await libraryApi.scan(key, full, confirmation);
        await refresh();
        showMessage(host, `${result.full ? "Vollscan" : "Scan"} ${result.id} ist dauerhaft eingereiht.`);
        void pollScanStatus(refresh);
      } finally {
        button.disabled = false;
        button.textContent = label;
      }
    }));
  });
}

export function libraryRow(library: ManagedLibrary): string {
  const latest = library.latestScanStatus
    ? `${library.latestScanStatus}${library.latestScanStartedAtUnixMs ? ` · ${new Date(library.latestScanStartedAtUnixMs).toLocaleString()}` : ""}`
    : "noch nicht gescannt";
  const status = library.enabled ? "AKTIV" : "DEAKTIVIERT";
  const scanning = library.latestScanStatus === "queued" || library.latestScanStatus === "running";
  const blocked = library.storage.state === "blocked";
  const capacity = library.storage.availableBytes === null || library.storage.totalBytes === null
    ? "Kapazität unbekannt"
    : `${formatBytes(library.storage.availableBytes)} von ${formatBytes(library.storage.totalBytes)} frei`;
  const reasons = library.storage.reasons.length === 0
    ? "Pfad erreichbar, lesbar und schreibbar"
    : library.storage.reasons.map((reason) => storageReason(reason.code)).join(" · ");
  return `<article class="library-row${library.enabled ? "" : " disabled"}">
    <div class="library-identity"><strong>${escapeMarkup(library.name)}</strong><small>${escapeMarkup(library.key)} · ${escapeMarkup(library.filesystemPath)}</small><small>${library.fileCount} Dateien · ${library.missingFileCount} fehlend · ${escapeMarkup(latest)}</small><small>${escapeMarkup(capacity)} · ${escapeMarkup(reasons)} · geprüft ${new Date(library.storage.checkedAtUnixMs).toLocaleString()}</small></div>
    <span class="role-badge storage-${library.storage.state}">${status} · ${storageState(library.storage.state)}</span>
    <form data-library-edit="${escapeAttribute(library.key)}" data-revision="${library.revision}"><label>Bibliothek umbenennen<input name="name" aria-label="Anzeigename für ${escapeAttribute(library.name)}" value="${escapeAttribute(library.name)}" required maxlength="160"></label><button class="secondary-action">Umbenennen</button></form>
    <div class="library-actions">
      <button class="secondary-action" data-library-scan="${escapeAttribute(library.key)}" data-full="false"${library.enabled && !scanning && !blocked ? "" : " disabled"}>${scanning ? "Scan läuft …" : "Scan"}</button>
      <button class="secondary-action" data-library-scan="${escapeAttribute(library.key)}" data-full="true"${library.enabled && !scanning && !blocked ? "" : " disabled"}>Vollscan</button>
      <button class="secondary-action" data-library-toggle="${escapeAttribute(library.key)}" data-revision="${library.revision}" data-enabled="${library.enabled}">${library.enabled ? "Deaktivieren" : "Aktivieren"}</button>
    </div>
  </article>`;
}

function storageState(state: ManagedLibrary["storage"]["state"]): string {
  if (state === "healthy") return "GESUND";
  if (state === "degraded") return "EINGESCHRÄNKT";
  return "BLOCKIERT";
}

function storageReason(code: string): string {
  const reasons: Record<string, string> = {
    storage_unreachable: "Pfad nicht erreichbar",
    storage_not_directory: "Pfad ist kein Verzeichnis",
    storage_unreadable: "nicht lesbar",
    storage_not_writable: "nicht schreibbar",
    storage_capacity_blocked: "weniger als 1 GiB frei",
    storage_capacity_low: "Speicher fast voll",
    storage_capacity_unknown: "Kapazität nicht messbar",
  };
  return reasons[code] ?? code;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  return `${bytes} B`;
}

async function pollScanStatus(refresh: () => Promise<void>): Promise<void> {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 2000));
    try {
      const libraries = await libraryApi.list();
      await refresh();
      if (!libraries.some((library) => library.latestScanStatus === "queued" || library.latestScanStatus === "running")) return;
    } catch {
      return;
    }
  }
}

const pendingLibraryActions = new WeakSet<HTMLElement>();

async function validateLibraryPath(path: string) {
  try {
    return await libraryApi.validatePath(path);
  } catch (error) {
    if (error instanceof ApiError && error.status === 400) throw new Error("Der Bibliothekspfad ist ungültig, fehlt oder ist nicht zugreifbar.");
    throw error;
  }
}

async function run(host: HTMLElement, action: () => Promise<void>): Promise<void> {
  if (pendingLibraryActions.has(host)) return;
  pendingLibraryActions.add(host);
  try {
    await action();
  } catch (error) {
    showMessage(host, error instanceof Error ? error.message : "Vorgang fehlgeschlagen", true);
  } finally {
    pendingLibraryActions.delete(host);
  }
}

function showValidation(host: HTMLElement, text: string): void {
  const element = host.querySelector<HTMLElement>("#library-validation");
  if (element) element.textContent = text;
}

function showMessage(host: HTMLElement, text: string, error = false): void {
  const element = host.querySelector<HTMLElement>("#admin-message");
  if (!element) return;
  element.textContent = text;
  element.classList.toggle("error", error);
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function escapeAttribute(value: string): string {
  return escapeMarkup(value).replaceAll("'", "&#39;");
}
