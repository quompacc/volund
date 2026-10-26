/** Personal API tokens in "Konto" (ADR 0037). */
export interface ApiToken {
  id: string;
  name: string;
  prefix: string;
  scope: "read" | "catalog-write";
  createdAtUnixMs: number;
  expiresAtUnixMs: number;
  lastUsedAtUnixMs: number | null;
}

export interface CreatedApiToken extends ApiToken { token: string; }

export interface ApiTokenSources {
  list: () => Promise<ApiToken[]>;
  create: (name: string, scope: ApiToken["scope"], expiresInDays: number) => Promise<CreatedApiToken>;
  revoke: (id: string) => Promise<void>;
}

const SCOPE_LABEL = { read: "Nur lesen", "catalog-write": "Katalog bearbeiten" } as const;

export function apiTokenPanel(tokens: ApiToken[], canWrite: boolean, locale = "de-DE"): string {
  const date = (value: number) => new Date(value).toLocaleDateString(locale);
  const rows = tokens.length === 0
    ? '<p class="empty-copy">Noch kein API-Token angelegt.</p>'
    : `<ul class="api-token-list">${tokens.map((token) => `<li><div><strong>${escapeMarkup(token.name)}</strong>
        <small><code>vlnd_${escapeMarkup(token.prefix)}…</code> · ${SCOPE_LABEL[token.scope]} · gültig bis ${date(token.expiresAtUnixMs)} · ${token.lastUsedAtUnixMs ? `zuletzt genutzt ${date(token.lastUsedAtUnixMs)}` : "noch nicht genutzt"}</small></div>
        <button type="button" class="secondary-action" data-revoke-token="${escapeMarkup(token.id)}">Widerrufen</button></li>`).join("")}</ul>`;
  return `<section class="admin-section" id="api-tokens"><div class="section-heading"><div><p class="eyebrow">AUTOMATISIERUNG</p><h2>API-Tokens</h2></div><span>${tokens.length} aktiv</span></div>
    <p class="admin-copy">Für Skripte und Werkzeuge: <code>Authorization: Bearer vlnd_…</code>. Tokens verwalten weder Konten noch Sitzungen und reichen nie über deine Rolle hinaus.</p>
    <form id="create-api-token" class="admin-form">
      <label><span>Name</span><input name="name" required maxlength="80" placeholder="z. B. Backup-Skript"></label>
      <label><span>Berechtigung</span><select name="scope"><option value="read">Nur lesen</option>${canWrite ? '<option value="catalog-write">Katalog bearbeiten</option>' : ""}</select></label>
      <label><span>Gültigkeit</span><select name="days"><option value="30">30 Tage</option><option value="90" selected>90 Tage</option><option value="365">365 Tage</option></select></label>
      <button class="secondary-action">Token erzeugen</button>
    </form>
    <div id="api-token-secret" aria-live="polite"></div>
    ${rows}</section>`;
}

export function secretMarkup(created: CreatedApiToken): string {
  return `<div class="api-token-secret"><p><strong>Neues Token „${escapeMarkup(created.name)}“ – jetzt kopieren, es wird nicht noch einmal angezeigt:</strong></p>
    <p><code data-token-secret>${escapeMarkup(created.token)}</code> <button type="button" class="secondary-action" data-copy-token>Kopieren</button></p></div>`;
}

/**
 * Wire creation, one-time display, copy, and revocation. `host` is the freshly
 * rendered token section; `root` survives `refresh` and receives the secret.
 */
export function bindApiTokens(
  host: HTMLElement,
  root: HTMLElement,
  sources: ApiTokenSources,
  refresh: () => Promise<void>,
  message: (text: string) => void,
  win: Window = window,
): void {
  const form = host.querySelector<HTMLFormElement>("#create-api-token");
  form?.addEventListener("submit", (event) => {
    event.preventDefault();
    const data = new FormData(form);
    void sources.create(String(data.get("name")).trim(), String(data.get("scope")) as ApiToken["scope"], Number(data.get("days")))
      .then(async (created) => {
        await refresh();
        const target = root.querySelector<HTMLElement>("#api-token-secret");
        if (target) target.innerHTML = secretMarkup(created);
        message("API-Token erzeugt.");
      }, (error: Error) => message(error.message));
  });
  host.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    if (target.closest("[data-copy-token]")) {
      const secret = root.querySelector<HTMLElement>("[data-token-secret]")?.textContent ?? "";
      void win.navigator.clipboard?.writeText(secret).then(() => message("Token in die Zwischenablage kopiert."), () => message("Kopieren nicht möglich; bitte markieren und kopieren."));
      return;
    }
    const revoke = target.closest<HTMLButtonElement>("[data-revoke-token]");
    if (!revoke) return;
    if (!win.confirm("Token widerrufen? Skripte, die es nutzen, verlieren sofort den Zugriff.")) return;
    void sources.revoke(revoke.dataset.revokeToken!).then(async () => { await refresh(); message("API-Token widerrufen."); }, (error: Error) => message(error.message));
  });
}

function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
