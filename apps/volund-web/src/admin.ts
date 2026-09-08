import { catalogApi, identityApi, jobsApi, operationsApi, policyApi } from "./api";
import { confirmExact } from "./confirmation";
import { bindJobActions, jobPanel } from "./jobs-view";
import { bindLibraryActions, libraryPanel } from "./libraries";
import { bindOperationsActions, operationsPanel } from "./operations-view";
import { settingString } from "./preferences";
import { bindPolicyActions, policyPanels } from "./policy-view";
import { bindLifecycleEntries, lifecycleEntry } from "./lifecycle-ui";
import { formatBytes } from "./format";
import type { CurrentSession, ManagedUser, OwnSession, Page, QuarantineSummary, Setting, UserPreferences } from "./types";

export function mountAdministration(host: HTMLElement, actor: CurrentSession): () => void {
  let disposed = false;
  let activeArea = "account";
  let jobFilters: { kind?: string; status?: string; offset?: number } = {};
  let quarantineOffset = 0;
  host.innerHTML = `<div class="page-scroll"><header class="page-hero"><div><p class="eyebrow">INSTANZVERWALTUNG</p>
    <h1>Administration</h1><p>Nutzer, Sitzungen und code-definierte Instanzeinstellungen zentral verwalten.</p></div></header>
    <div id="admin-content" class="admin-content"><p class="loading-copy">Administration wird geladen …</p></div></div>`;
  const refresh = async (): Promise<void> => {
    if (disposed) return;
    const content = host.querySelector<HTMLElement>("#admin-content");
    if (!content) return;
    try {
      const canAdminister = actor.role === "owner" || actor.role === "administrator";
      let [sessions, settings, preferences, users, operations, jobs, profiles, schedules, retention, quarantines] = await Promise.all([
        identityApi.sessions(),
        identityApi.settings(),
        identityApi.preferences(),
        canAdminister ? identityApi.users() : Promise.resolve([]),
        canAdminister ? operationsApi.health() : Promise.resolve(null),
        canAdminister ? jobsApi.list(jobFilters) : Promise.resolve(null),
        canAdminister ? policyApi.profiles() : Promise.resolve([]),
        canAdminister ? policyApi.schedules() : Promise.resolve([]),
        canAdminister ? policyApi.retentionPreview() : Promise.resolve(null),
        canAdminister ? catalogQuarantines(quarantineOffset) : Promise.resolve(null),
      ]);
      if (disposed) return;
      if (jobs && jobs.items.length === 0 && jobs.offset > 0 && jobs.offset >= jobs.total) {
        const offset = jobs.total === 0 ? 0 : Math.floor((jobs.total - 1) / jobs.limit) * jobs.limit;
        jobs = await jobsApi.list({ ...jobFilters, offset });
      }
      if (quarantines && quarantines.items.length === 0 && quarantines.offset > 0 && quarantines.offset >= quarantines.total) {
        const offset = quarantines.total === 0 ? 0 : Math.floor((quarantines.total - 1) / quarantines.limit) * quarantines.limit;
        quarantines = await catalogQuarantines(offset);
      }
      if (disposed) return;
      if (jobs) jobFilters = { ...jobFilters, offset: jobs.offset };
      if (quarantines) quarantineOffset = quarantines.offset;
      const locale = settingString(settings, "instance.locale", "de-DE");
      const timeZone = settingString(settings, "instance.timeZone", "Europe/Berlin");
      const areas = [
        { id: "account", label: "Konto", content: `${accountPanel(actor)}${preferencesPanel(preferences)}${sessionPanel(sessions, locale, timeZone)}` },
        ...(canAdminister ? [
          { id: "operations", label: "Betrieb", content: `${operations ? operationsPanel(operations, locale, timeZone) : ""}${jobs ? jobPanel(jobs, jobFilters, locale, timeZone) : ""}` },
          { id: "storage", label: "Speicher", content: `${operations ? libraryPanel(operations.libraries) : ""}${quarantinePanel(quarantines, actor)}` },
          { id: "automation", label: "Automatisierung", content: retention && operations ? policyPanels(profiles, schedules, retention, operations.libraries, timeZone) : "" },
          { id: "access", label: "Zugriff", content: userPanel(users, actor) },
          { id: "settings", label: "Einstellungen", content: settingsPanel(settings) },
        ] : []),
      ];
      content.innerHTML = `<div id="admin-message" class="admin-message" role="status" aria-live="polite"></div>${adminAreaMarkup(areas, activeArea)}`;
      bindAdminAreas(content, (area) => { activeArea = area; });
      bindActions(content, refresh);
      if (canAdminister) bindOperationsActions(content);
      if (canAdminister) bindLibraryActions(content, refresh);
      if (canAdminister) bindJobActions(content, (filters) => { jobFilters = filters; }, refresh);
      if (canAdminister) bindPolicyActions(content, refresh);
      if (canAdminister) bindLifecycleEntries(content, () => void refresh());
      content.querySelectorAll<HTMLButtonElement>("[data-quarantine-page]").forEach((button) => {
        button.addEventListener("click", () => {
          if (button.disabled) return;
          button.disabled = true;
          quarantineOffset = Number(button.dataset.quarantineOffset);
          void refresh();
        });
      });
    } catch (error) {
      if (disposed) return;
      content.innerHTML = `<div class="admin-message error" role="alert">${escapeMarkup(message(error))}</div><button id="admin-retry" type="button" class="secondary-action">Erneut versuchen</button>`;
      const retry = content.querySelector<HTMLButtonElement>("#admin-retry")!;
      retry.addEventListener("click", () => {
        if (retry.disabled) return;
        retry.disabled = true;
        retry.textContent = "Administration wird geladen …";
        void refresh();
      });
    }
  };
  void refresh();
  return () => { disposed = true; };
}

function adminAreaMarkup(areas: { id: string; label: string; content: string }[], activeArea: string): string {
  return `<nav class="admin-area-nav" role="tablist" aria-label="Administrationsbereiche">${areas.map((area) => `<button type="button" role="tab" id="admin-tab-${area.id}" aria-controls="admin-panel-${area.id}" aria-selected="${area.id === activeArea}" tabindex="${area.id === activeArea ? 0 : -1}" data-admin-area="${area.id}">${area.label}</button>`).join("")}</nav>${areas.map((area) => `<div class="admin-area-panel" role="tabpanel" id="admin-panel-${area.id}" aria-labelledby="admin-tab-${area.id}" data-admin-panel="${area.id}"${area.id === activeArea ? "" : " hidden"}>${area.content}</div>`).join("")}`;
}

function bindAdminAreas(host: HTMLElement, selectArea: (area: string) => void): void {
  const tabs = [...host.querySelectorAll<HTMLButtonElement>("[data-admin-area]")];
  const activate = (tab: HTMLButtonElement): void => {
    const area = tab.dataset.adminArea!;
    tabs.forEach((candidate) => {
      const selected = candidate === tab;
      candidate.setAttribute("aria-selected", String(selected));
      candidate.tabIndex = selected ? 0 : -1;
    });
    host.querySelectorAll<HTMLElement>("[data-admin-panel]").forEach((panel) => { panel.hidden = panel.dataset.adminPanel !== area; });
    selectArea(area);
  };
  tabs.forEach((tab, index) => {
    tab.addEventListener("click", () => activate(tab));
    tab.addEventListener("keydown", (event) => {
      const target = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : event.key === "ArrowRight" ? (index + 1) % tabs.length : event.key === "ArrowLeft" ? (index - 1 + tabs.length) % tabs.length : -1;
      if (target < 0) return;
      event.preventDefault();
      activate(tabs[target]!);
      tabs[target]!.focus();
    });
  });
}

async function catalogQuarantines(offset: number): Promise<Page<QuarantineSummary> | null> {
  try { return await catalogApi.quarantines(offset); }
  catch { return null; }
}

function quarantinePanel(page: Page<QuarantineSummary> | null, actor: CurrentSession): string {
  if (actor.role !== "owner" && actor.role !== "administrator") return "";
  if (page === null) return '<section class="admin-section"><h2>Quarantäne & Wiederherstellung</h2><p class="admin-message error" role="alert">Quarantäne konnte nicht geladen werden. Der Bestand ist unbekannt. Bitte lade die Administration erneut.</p></section>';
  const items = page.items;
  const rows = items.length === 0 ? '<div class="empty-concept"><p>Keine Originaldateien befinden sich in Quarantäne.</p></div>' : items.map((item) => `<article><div><strong>${escapeMarkup(item.relativePath)}</strong><small>${escapeMarkup(item.libraryName)} · ${formatBytes(item.byteSize)} · SHA-256 ${escapeMarkup(item.sha256)}</small><small>Aufbewahrung bis ${new Intl.DateTimeFormat("de-DE", { dateStyle: "medium", timeStyle: "short" }).format(new Date(item.retentionUntilUnixMs))}</small></div>${lifecycleEntry("source.recover", item.sourceId, item.revision, "Wiederherstellen")}${actor.role === "owner" && item.retentionExpired ? lifecycleEntry("source.purge", item.sourceId, item.revision, "Endgültig löschen") : ""}</article>`).join("");
  const pagination = page.total > page.limit ? `<nav class="pagination" aria-label="Quarantäneseiten"><button type="button" class="secondary-action" data-quarantine-page="previous" data-quarantine-offset="${Math.max(0, page.offset - page.limit)}"${page.offset === 0 ? " disabled" : ""}>← Zurück</button><span>${page.offset + 1}–${Math.min(page.offset + page.limit, page.total)} von ${page.total}</span><button type="button" class="secondary-action" data-quarantine-page="next" data-quarantine-offset="${page.offset + page.limit}"${page.offset + page.limit >= page.total ? " disabled" : ""}>Weiter →</button></nav>` : "";
  return `<section class="admin-section"><div class="section-heading"><div><p class="eyebrow">ORIGINALDATEIEN</p><h2>Quarantäne & Wiederherstellung</h2></div><span>${page.total}</span></div><p class="admin-copy">Wiederherstellung überschreibt niemals vorhandene Dateien. Endgültiges Löschen erscheint nur für Owner nach Ablauf der Aufbewahrung.</p><div class="admin-list quarantine-list">${rows}</div>${pagination}</section>`;
}

function bindActions(host: HTMLElement, refresh: () => Promise<void>): void {
  host.querySelector<HTMLFormElement>("#user-preferences")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement;
    const current = JSON.parse(form.dataset.preferences!) as UserPreferences;
    const data = new FormData(form);
    const status = form.querySelector<HTMLElement>(".form-message")!;
    const button = form.querySelector<HTMLButtonElement>("button")!;
    if (button.disabled) return;
    button.disabled = true; status.textContent = "Persönliche Einstellungen werden gespeichert …";
    void identityApi.updatePreferences({ ...current,
      previewAutoLoad: String(data.get("previewAutoLoad")) as UserPreferences["previewAutoLoad"],
      background: String(data.get("background")) as UserPreferences["background"],
      gridVisible: data.has("gridVisible"), contrast: String(data.get("contrast")) as UserPreferences["contrast"],
      renderStyle: String(data.get("renderStyle")) as UserPreferences["renderStyle"],
      problemMinimumSeverity: String(data.get("problemMinimumSeverity")) as UserPreferences["problemMinimumSeverity"],
    }).then(async () => {
      await refresh();
      host.querySelector<HTMLElement>("#user-preferences .form-message")!.textContent = "Persönliche Einstellungen gespeichert.";
    }).catch((error: unknown) => {
      button.disabled = false; status.classList.add("error"); status.textContent = message(error);
    });
  });
  host.querySelector<HTMLFormElement>("#create-user")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget as HTMLFormElement);
    void run(host, async () => {
      await identityApi.createUser({
        email: String(data.get("email")),
        displayName: String(data.get("displayName")),
        role: String(data.get("role")),
        password: String(data.get("password")),
      });
      await refresh();
    });
  });
  host.querySelector<HTMLFormElement>("#invite-user")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget as HTMLFormElement);
    void run(host, async () => {
      const invitation = await identityApi.inviteUser({
        email: String(data.get("email")),
        displayName: String(data.get("displayName")),
        role: String(data.get("role")),
      });
      await refresh();
      showMessage(host, `Einladungslink (einmalig, bis ${new Date(invitation.expiresAtUnixMs).toLocaleString()}): ${activationUrl(invitation.activationToken, window.location.origin)}`);
    });
  });
  host.querySelector<HTMLFormElement>("#change-own-password")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement;
    const data = new FormData(form);
    void run(host, async () => {
      const password = String(data.get("newPassword"));
      if (password !== String(data.get("confirmation"))) throw new Error("Die neuen Passwörter stimmen nicht überein.");
      await identityApi.changeOwnPassword(String(data.get("currentPassword")), password);
      form.reset();
      showMessage(host, "Passwort geändert; andere Sitzungen wurden widerrufen.");
    });
  });
  host.querySelectorAll<HTMLButtonElement>("[data-user-status]").forEach((button) => {
    button.addEventListener("click", () => void run(host, async () => {
      await confirmExact(
        `SET USER STATUS ${button.dataset.userStatus} ${button.dataset.nextStatus}`,
        "Der Kontostatus wird sofort wirksam; deaktivierte oder gesperrte Konten verlieren den Zugriff.",
      );
      await identityApi.updateUser(button.dataset.userStatus!, { status: button.dataset.nextStatus! });
      await refresh();
    }));
  });
  host.querySelectorAll<HTMLSelectElement>("[data-user-role]").forEach((select) => {
    select.addEventListener("change", () => void run(host, async () => {
      const previousRole = select.dataset.currentRole!;
      try {
        await confirmExact(
          `SET USER ROLE ${select.dataset.userRole} ${select.value}`,
          "Die neue Rolle verändert den Zugriff dieses Kontos sofort.",
        );
        await identityApi.updateUser(select.dataset.userRole!, { role: select.value });
        await refresh();
      } catch (error) {
        select.value = previousRole;
        throw error;
      }
    }));
  });
  host.querySelectorAll<HTMLButtonElement>("[data-reset-password]").forEach((button) => {
    button.addEventListener("click", () => {
      if (host.dataset.accountPending === "true") return;
      const password = window.prompt("Neues Passwort (mindestens 12 Zeichen)");
      if (!password) return;
      void run(host, async () => {
        await identityApi.resetPassword(button.dataset.resetPassword!, password);
        showMessage(host, "Passwort ersetzt; bestehende Sitzungen wurden widerrufen.");
      });
    });
  });
  host.querySelectorAll<HTMLButtonElement>("[data-revoke-session]").forEach((button) => {
    button.addEventListener("click", () => void run(host, async () => {
      await confirmExact(
        `REVOKE SESSION ${button.dataset.revokeSession}`,
        "Die ausgewählte Browsersitzung wird sofort abgemeldet.",
      );
      await identityApi.revokeSession(button.dataset.revokeSession!);
      await refresh();
    }));
  });
  host.querySelectorAll<HTMLFormElement>("[data-setting]").forEach((form) => {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const raw = String(new FormData(form).get("value"));
      const setting = JSON.parse(form.dataset.setting!) as Setting;
      const status = form.querySelector<HTMLElement>("[data-setting-status]")!;
      const button = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
      if (button.disabled) return;
      void (async () => {
        button.disabled = true;
        status.classList.remove("error");
        status.textContent = "Einstellung wird gespeichert …";
        const confirmation = ["jobs", "retention"].includes(setting.domain)
          ? await confirmExact(`APPLY POLICY ${setting.key}`, "Die Einstellung verändert unmittelbar die native Ausführungspolitik.")
          : undefined;
        await identityApi.updateSetting(setting, settingInputValue(setting, raw), confirmation);
        await refresh();
        const refreshed = [...host.querySelectorAll<HTMLFormElement>("[data-setting-key]")]
          .find((candidate) => candidate.dataset.settingKey === setting.key);
        if (refreshed) refreshed.querySelector<HTMLElement>("[data-setting-status]")!.textContent = "Gespeichert und wirksam.";
      })().catch((error: unknown) => {
        button.disabled = false;
        status.classList.add("error");
        status.textContent = message(error);
      });
    });
  });
}

function accountPanel(actor: CurrentSession): string {
  return `<section class="admin-section"><div class="section-heading"><div><p class="eyebrow">KONTO</p><h2>${escapeMarkup(actor.displayName)}</h2></div><span class="role-badge">${escapeMarkup(actor.role)}</span></div>
    <p class="admin-copy">${escapeMarkup(actor.email)}</p><form id="change-own-password" class="admin-form"><label><span>Aktuelles Passwort</span><input name="currentPassword" type="password" autocomplete="current-password" required></label><label><span>Neues Passwort</span><input name="newPassword" type="password" autocomplete="new-password" minlength="12" required></label><label><span>Passwort bestätigen</span><input name="confirmation" type="password" autocomplete="new-password" minlength="12" required></label><button class="secondary-action">Eigenes Passwort ändern</button></form></section>`;
}

function preferencesPanel(preferences: UserPreferences): string {
  const option = (value: string, current: string, label: string): string => `<option value="${value}"${value === current ? " selected" : ""}>${label}</option>`;
  return `<section class="admin-section"><div class="section-heading"><div><p class="eyebrow">PERSÖNLICH</p><h2>Vorschau & Problemanzeige</h2><p>Diese Werte gelten nur für dein angemeldetes Konto und verändern keine Sicherheits- oder Servergrenzen.</p></div><span>Revision ${preferences.revision}</span></div><form id="user-preferences" class="preference-grid" data-preferences='${escapeAttribute(JSON.stringify(preferences))}'>
    <label>Vorschauen automatisch laden<select name="previewAutoLoad">${option("manual", preferences.previewAutoLoad, "Nur auf Anforderung")}${option("selected", preferences.previewAutoLoad, "Ausgewählte Datei")}${option("visible", preferences.previewAutoLoad, "Sichtbare Dateien")}</select><small>„Sichtbar“ kann auf großen Modellen mehr Browser-Ressourcen benötigen.</small></label>
    <label>Hintergrund<select name="background">${option("dark", preferences.background, "Dunkel")}${option("light", preferences.background, "Hell")}${option("system", preferences.background, "Systemvorgabe")}</select></label>
    <label>Kontrast<select name="contrast">${option("balanced", preferences.contrast, "Ausgewogen")}${option("high", preferences.contrast, "Hoch")}</select></label>
    <label>Darstellung<select name="renderStyle">${option("solid", preferences.renderStyle, "Flächen")}${option("wireframe", preferences.renderStyle, "Drahtmodell")}</select></label>
    <label>Mindestschwere im Problemcenter<select name="problemMinimumSeverity">${option("info", preferences.problemMinimumSeverity, "Information")}${option("warning", preferences.problemMinimumSeverity, "Warnung")}${option("error", preferences.problemMinimumSeverity, "Fehler")}</select></label>
    <label class="checkbox-label"><input type="checkbox" name="gridVisible"${preferences.gridVisible ? " checked" : ""}> Raster in 3D-Ansichten anzeigen</label>
    <button class="primary-action" type="submit">Persönliche Einstellungen speichern</button><p class="form-message" aria-live="polite"></p></form></section>`;
}

function sessionPanel(sessions: OwnSession[], locale: string, timeZone: string): string {
  return `<section class="admin-section"><div class="section-heading"><div><p class="eyebrow">SICHERHEIT</p><h2>Aktive Sitzungen</h2></div><span>${sessions.length}</span></div>
    <div class="admin-list">${sessions.map((session) => `<article><div><strong>${session.current ? "Diese Sitzung" : escapeMarkup(session.userAgent || "Unbekannter Browser")}</strong>
      <small>${formatSessionTime(session.lastSeenAtUnixMs, locale, timeZone)}${session.clientAddress ? ` · ${escapeMarkup(session.clientAddress)}` : ""}</small></div>
      ${session.current ? '<span class="role-badge">AKTUELL</span>' : `<button class="secondary-action" data-revoke-session="${session.id}">Widerrufen</button>`}</article>`).join("")}</div></section>`;
}

function userPanel(users: ManagedUser[], actor: CurrentSession): string {
  const roles = availableRoles(actor.role);
  const options = roles.map((role) => `<option value="${role}">${role[0]!.toUpperCase()}${role.slice(1)}</option>`).join("");
  return `<section class="admin-section"><div class="section-heading"><div><p class="eyebrow">ZUGRIFF</p><h2>Nutzer</h2></div><span>${users.length}</span></div>
    <form id="create-user" class="admin-form" aria-label="Nutzer anlegen"><h3 class="admin-form-title">Nutzer direkt anlegen</h3><label><span>Name</span><input name="displayName" required maxlength="160"></label><label><span>E-Mail</span><input name="email" type="email" required></label>
      <label><span>Rolle</span><select name="role">${options}</select></label>
      <label><span>Startpasswort</span><input name="password" type="password" minlength="12" required></label><button class="primary-action">Nutzer anlegen</button></form>
    <form id="invite-user" class="admin-form" aria-label="Nutzer einladen"><h3 class="admin-form-title">Nutzer per Link einladen</h3><label><span>Name</span><input name="displayName" required maxlength="160"></label><label><span>E-Mail</span><input name="email" type="email" required></label>
      <label><span>Rolle</span><select name="role">${options}</select></label><button class="secondary-action">Einladungslink erzeugen</button></form>
    <div class="admin-list">${users.map((user) => userRow(user, actor)).join("")}</div></section>`;
}

function userRow(user: ManagedUser, actor: CurrentSession): string {
  const roles = availableRoles(actor.role);
  if (!roles.includes(user.role)) roles.push(user.role);
  const own = user.id === actor.userId;
  const protectedOwner = actor.role === "administrator" && user.role === "owner";
  const protectedAccount = own || protectedOwner;
  const action = userStatusAction(user.status);
  return `<article><div><strong>${escapeMarkup(user.displayName)}</strong><small>${escapeMarkup(user.email)} · ${escapeMarkup(user.status)}${user.mustChangePassword ? " · Passwortwechsel erforderlich" : ""}</small></div>
    <select data-user-role="${user.id}" data-current-role="${user.role}" aria-label="Rolle für ${escapeAttribute(user.displayName)}"${protectedAccount ? ' disabled title="Dieses geschützte Konto kann hier nicht geändert werden"' : ""}>${roles.map((role) => `<option value="${role}"${role === user.role ? " selected" : ""}>${role}</option>`).join("")}</select>
    <button class="secondary-action" data-reset-password="${user.id}"${protectedOwner ? ' disabled title="Owner können nur durch einen Owner geändert werden"' : ""}>Passwort</button>
    <button class="secondary-action" data-user-status="${user.id}" data-next-status="${action.next}"${protectedAccount ? ' disabled title="Dieses geschützte Konto kann hier nicht geändert werden"' : ""}>${own ? "Aktuelles Konto" : protectedOwner ? "Owner-geschützt" : action.label}</button></article>`;
}

export function availableRoles(actorRole: CurrentSession["role"]): CurrentSession["role"][] {
  const roles: CurrentSession["role"][] = ["viewer", "editor", "administrator"];
  if (actorRole === "owner") roles.push("owner");
  return roles;
}

export function userStatusAction(status: ManagedUser["status"]): { next: "active" | "disabled"; label: string } {
  if (status === "active") return { next: "disabled", label: "Deaktivieren" };
  if (status === "locked") return { next: "active", label: "Entsperren" };
  if (status === "invited") return { next: "disabled", label: "Einladung sperren" };
  return { next: "active", label: "Aktivieren" };
}

export function activationUrl(token: string, origin: string): string {
  const url = new URL("/", origin);
  url.searchParams.set("invite", token);
  return url.toString();
}

function settingsPanel(settings: Setting[]): string {
  const domains = [...new Set(settings.map((setting) => setting.domain))];
  return `<section class="admin-section settings-section"><div class="section-heading"><div><p class="eyebrow">KONFIGURATION</p><h2>Instanzeinstellungen</h2><p>Wirksame Werte, Herkunft und Auswirkungen sind pro Bereich erklärt. Technische Schlüssel bleiben zur Diagnose sichtbar.</p></div><span>${settings.length} registriert</span></div>
    <div class="settings-groups">${domains.map((domain, index) => `<details class="settings-group"${index === 0 ? " open" : ""}><summary><span>${escapeMarkup(domainCopy(domain).label)}</span><small>${escapeMarkup(domainCopy(domain).description)}</small></summary><div class="setting-grid">${settings.filter((setting) => setting.domain === domain).map(settingControl).join("")}</div></details>`).join("")}</div></section>`;
}

export function settingControl(setting: Setting): string {
  const copy = settingCopy(setting.key);
  const metadata = `${originLabel(setting.origin)} · ${setting.effect === "restart" ? "Neustart erforderlich" : "Sofort wirksam"} · Revision ${setting.revision}`;
  if (!setting.editable) {
    const value = setting.sensitive
      ? (setting.configured ? "Konfiguriert" : "Nicht konfiguriert")
      : formatSettingValue(setting);
    const owner = setting.constraints.compiled ? "durch Anwendung festgelegt" : "durch Betreiber verwaltet";
    return `<article class="setting-card"><div><h4>${escapeMarkup(copy.label)}</h4><p>${escapeMarkup(copy.description)}</p><code>${escapeMarkup(setting.key)}</code><small>${escapeMarkup(metadata)} · ${owner}</small></div><strong>${escapeMarkup(value)}</strong></article>`;
  }
  const control = setting.valueType === "boolean"
    ? `<select name="value" aria-label="${escapeAttribute(setting.key)}"><option value="true"${setting.value === true ? " selected" : ""}>Aktiviert</option><option value="false"${setting.value === false ? " selected" : ""}>Deaktiviert</option></select>`
    : setting.constraints.allowed
    ? `<select name="value" aria-label="${escapeAttribute(setting.key)}">${setting.constraints.allowed.map((value) => `<option value="${escapeAttribute(value)}"${value === setting.value ? " selected" : ""}>${escapeMarkup(settingOptionLabel(value))}</option>`).join("")}</select>`
    : `<input name="value" aria-label="${escapeAttribute(setting.key)}" value="${escapeAttribute(String(setting.value))}"${setting.valueType === "integer" ? ` type="number" step="1"${setting.constraints.minimum === undefined ? "" : ` min="${setting.constraints.minimum}"`}${setting.constraints.maximum === undefined ? "" : ` max="${setting.constraints.maximum}"`}` : ""} required>`;
  return `<form class="setting-card" data-setting-key="${escapeAttribute(setting.key)}" data-setting='${escapeAttribute(JSON.stringify(setting))}'><label><span>${escapeMarkup(copy.label)}</span><small>${escapeMarkup(copy.description)}</small><code>${escapeMarkup(setting.key)}</code><small>Aktuell: ${escapeMarkup(formatSettingValue(setting))} · ${escapeMarkup(metadata)}</small>${control}</label><button class="secondary-action" type="submit">Speichern</button><p class="form-message" data-setting-status aria-live="polite"></p></form>`;
}

const SETTING_COPY: Record<string, { label: string; description: string }> = {
  "instance.name": { label: "Name der Instanz", description: "Wird in der Oberfläche als Name dieses VÖLUND-Archivs verwendet." },
  "instance.locale": { label: "Sprache und Zahlenformat", description: "Bestimmt die Darstellung lokalisierter Texte, Zahlen und Daten." },
  "instance.timeZone": { label: "Zeitzone", description: "Wird für Zeitangaben, Zeitpläne und Verlaufseinträge verwendet." },
  "catalog.defaultView": { label: "Startansicht des Katalogs", description: "Legt fest, ob Modelle oder Rohdateien zuerst erscheinen." },
  "security.sessionIdleMinutes": { label: "Inaktive Sitzung beenden", description: "Meldet Nutzer nach dieser Zeit ohne Aktivität ab." },
  "security.sessionAbsoluteHours": { label: "Maximale Sitzungsdauer", description: "Beendet eine Sitzung unabhängig von der Aktivität spätestens nach dieser Dauer." },
  "imports.defaultModelKind": { label: "Standardtyp bei Importen", description: "Vorauswahl für neue Modelle im Importassistenten." },
  "imports.draftRetentionDays": { label: "Importentwürfe aufbewahren", description: "Entfernt nicht veröffentlichte Entwürfe erst nach dieser Frist." },
  "imports.incomingCapacityBytes": { label: "Kapazität für eingehende Dateien", description: "Gemeinsames Größenlimit für noch nicht veröffentlichte Uploads." },
  "imports.maxConcurrentUploads": { label: "Gleichzeitige Uploads", description: "Begrenzt parallele Dateiübertragungen pro nativer Instanz." },
  "previews.defaultProfile": { label: "Standardprofil für Vorschauen", description: "Steuert Geschwindigkeit und Detailgrad neu erzeugter Vorschauen." },
  "thumbnails.defaultSource": { label: "Bevorzugte Vorschaubildquelle", description: "Bestimmt, welche verfügbare Quelle zuerst angezeigt wird." },
  "jobs.scanConcurrency": { label: "Parallele Scans", description: "Maximale Zahl gleichzeitig laufender Bibliotheksscans." },
  "jobs.conversionConcurrency": { label: "Parallele Konvertierungen", description: "Maximale Zahl gleichzeitig laufender nativer Vorschaukonvertierungen." },
  "retention.derivedArtifactDays": { label: "Abgeleitete Dateien aufbewahren", description: "Altersgrenze für nicht mehr benötigte Vorschauartefakte." },
  "retention.jobDiagnosticDays": { label: "Auftragsdiagnosen aufbewahren", description: "Altersgrenze für bereinigte Diagnoseinformationen." },
  "retention.maxArtifactRuns": { label: "Vorschauversionen je Quelle", description: "Maximale Zahl aufbewahrter Konvertierungsläufe pro Quelldatei." },
  "limits.importMaxFiles": { label: "Dateien je Import", description: "Feste Sicherheitsgrenze für die Anzahl von Dateien in einem Import." },
  "limits.importMaxBytes": { label: "Größe je Import", description: "Feste Sicherheitsgrenze für die Gesamtgröße eines Imports." },
  "limits.apiPageSize": { label: "Einträge je API-Seite", description: "Feste Obergrenze für paginierte API-Antworten." },
  "runtime.listenAddress": { label: "Dienstadresse", description: "Netzwerkadresse, an der der native Dienst lauscht." },
  "security.secureCookies": { label: "Sichere Browser-Cookies", description: "Erfordert HTTPS für Sitzungs- und CSRF-Cookies." },
  "database.connectionOverride": { label: "Datenbankverbindung", description: "Vom Betreiber gesetzte PostgreSQL-Verbindung; der geheime Wert wird nie angezeigt." },
  "security.bootstrapTokenFile": { label: "Bootstrap-Token-Datei", description: "Vom Betreiber verwaltete Datei für die einmalige Erstinitialisierung." },
};

function settingCopy(key: string): { label: string; description: string } {
  return SETTING_COPY[key] || { label: key.split(".").at(-1)!.replace(/([A-Z])/g, " $1"), description: "Registrierte, validierte Instanzeinstellung." };
}

function domainCopy(domain: string): { label: string; description: string } {
  return ({ instance: { label: "Instanz & Darstellung", description: "Name, Sprache und lokale Zeitdarstellung." }, catalog: { label: "Katalog", description: "Allgemeines Verhalten des Modellarchivs." }, security: { label: "Sicherheit & Sitzungen", description: "Anmeldung, Sitzungsgrenzen und Browser-Schutz." }, imports: { label: "Importe", description: "Vorgaben und Ressourcenlimits für sichere Veröffentlichungen." }, previews: { label: "Vorschauen", description: "Native Konvertierung und Standardqualität." }, thumbnails: { label: "Vorschaubilder", description: "Auswahl und Herkunft der Modellbilder." }, jobs: { label: "Auftragsausführung", description: "Parallelität und Grenzen nativer Hintergrundarbeit." }, retention: { label: "Aufbewahrung", description: "Regeln für abgeleitete Artefakte und Diagnosen." }, limits: { label: "Feste Schutzgrenzen", description: "Kompilierte Obergrenzen; nur lesbar." }, runtime: { label: "Native Laufzeit", description: "Vom Betreiber verwaltete Dienstparameter." }, database: { label: "Datenbank", description: "PostgreSQL-Verbindung und deren Herkunft." } } as Record<string, { label: string; description: string }>)[domain] || { label: domain, description: "Weitere registrierte Einstellungen." };
}

function originLabel(origin: Setting["origin"]): string {
  return ({ default: "Anwendungsstandard", persisted: "In PostgreSQL gespeichert", environment: "Betreiberumgebung" })[origin];
}

function settingOptionLabel(value: string): string {
  return ({ models: "Modelle", files: "Rohdateien", assembly: "Baugruppe", project: "Projekt", part: "Einzelteil",
    web: "Schnelle Web-Vorschau", fine: "Feine Vorschau", "primary-cad": "Primäre CAD-Datei",
    "image-first": "Bild bevorzugen" } as Record<string, string>)[value] || value;
}

export function formatSettingValue(setting: Setting): string {
  if (setting.sensitive) return setting.configured ? "Konfiguriert" : "Nicht konfiguriert";
  const unit = setting.constraints.unit;
  if (typeof setting.value === "number" && unit === "bytes") return formatBytes(setting.value);
  if (typeof setting.value === "number" && unit === "minutes") return `${setting.value} Minuten`;
  if (typeof setting.value === "number" && unit === "hours") return `${setting.value} Stunden`;
  if (typeof setting.value === "number" && unit === "days") return `${setting.value} Tage`;
  if (typeof setting.value === "number" && unit === "files") return `${setting.value.toLocaleString("de-DE")} Dateien`;
  if (typeof setting.value === "number" && unit === "items") return `${setting.value.toLocaleString("de-DE")} Einträge`;
  if (typeof setting.value === "boolean") return setting.value ? "Aktiviert" : "Deaktiviert";
  if (typeof setting.value === "string") return settingOptionLabel(setting.value);
  return String(setting.value ?? "—");
}

export function settingInputValue(setting: Setting, raw: string): unknown {
  if (setting.valueType === "integer") return Number(raw);
  if (setting.valueType === "boolean") return raw === "true";
  return raw;
}

export function formatSessionTime(timestamp: number, locale: string, timeZone: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short", timeZone }).format(timestamp);
}

async function run(host: HTMLElement, action: () => Promise<void>): Promise<void> {
  if (host.dataset.accountPending === "true") return;
  host.dataset.accountPending = "true";
  const controls = [...host.querySelectorAll<HTMLButtonElement | HTMLSelectElement>(
    "#create-user button, #invite-user button, #change-own-password button, [data-user-status], [data-user-role], [data-reset-password], [data-revoke-session]",
  )].filter((control) => !control.disabled);
  controls.forEach((control) => { control.disabled = true; });
  showMessage(host, "");
  try {
    await action();
  } catch (error) {
    showMessage(host, message(error), true);
  } finally {
    delete host.dataset.accountPending;
    controls.forEach((control) => { control.disabled = false; });
  }
}

function showMessage(host: HTMLElement, text: string, error = false): void {
  const element = host.querySelector<HTMLElement>("#admin-message");
  if (!element) return;
  element.textContent = text;
  element.classList.toggle("error", error);
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "Vorgang fehlgeschlagen";
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function escapeAttribute(value: string): string {
  return escapeMarkup(value).replaceAll("'", "&#39;");
}
