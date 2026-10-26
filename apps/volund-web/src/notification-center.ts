/** In-app notification center (ADR 0038). */
export interface AppNotification {
  key: string;
  category: "job" | "storage" | "backup" | "worker" | "security";
  kind: "condition" | "event";
  severity: "info" | "warning" | "error";
  title: string;
  message: string;
  occurredAtUnixMs: number;
  view: "problems" | "administration" | "model-problems";
  modelId: string | null;
  unread: boolean;
}

export interface NotificationPage {
  items: AppNotification[];
  attention: number;
  seenAtUnixMs: number | null;
}

export interface NotificationSources {
  list: () => Promise<NotificationPage>;
  markSeen: () => Promise<void>;
}

export const NOTIFICATION_REFRESH_MS = 60_000;

export function notificationToggleMarkup(): string {
  return '<button id="notifications-toggle" class="icon-button notification-toggle" type="button" aria-haspopup="true" aria-expanded="false" aria-controls="notification-panel" aria-label="Benachrichtigungen"><span class="icon icon-bell" aria-hidden="true"></span><b id="notification-count" hidden></b></button><div id="notification-panel" class="notification-panel" role="region" aria-label="Benachrichtigungen" hidden></div>';
}

export function notificationItemMarkup(item: AppNotification, locale = "de-DE"): string {
  const when = item.occurredAtUnixMs > 0 ? new Date(item.occurredAtUnixMs).toLocaleString(locale) : "";
  const state = item.kind === "condition" ? "besteht weiter" : item.unread ? "neu" : "gelesen";
  return `<li class="notification-item tone-${item.severity}${item.unread ? " unread" : ""}">
    <button type="button" data-notification-view="${item.view}"${item.modelId ? ` data-notification-model="${escapeMarkup(item.modelId)}"` : ""}>
      <strong>${escapeMarkup(item.title)}</strong><span>${escapeMarkup(item.message)}</span><small>${escapeMarkup(state)}${when ? ` · ${escapeMarkup(when)}` : ""}</small>
    </button></li>`;
}

export function notificationPanelMarkup(page: NotificationPage): string {
  if (page.items.length === 0) return '<p class="notification-empty">Keine Benachrichtigungen. Alles in Ordnung.</p>';
  return `<ul class="notification-list">${page.items.map((item) => notificationItemMarkup(item)).join("")}</ul>`;
}

/**
 * Keep the badge current and open the panel on demand. Opening marks events as
 * read; conditions stay until their cause is resolved.
 */
export function mountNotifications(
  toggle: HTMLButtonElement,
  panel: HTMLElement,
  sources: NotificationSources,
  open: (view: AppNotification["view"], modelId?: string) => void,
  win: Window = window,
): () => void {
  const badge = toggle.querySelector<HTMLElement>("#notification-count")!;
  let latest: NotificationPage | null = null;
  const showBadge = (count: number) => {
    badge.hidden = count === 0;
    badge.textContent = count > 99 ? "99+" : String(count);
    toggle.setAttribute("aria-label", count === 0 ? "Benachrichtigungen" : `Benachrichtigungen, ${count} erfordern Aufmerksamkeit`);
  };
  const load = async () => {
    try {
      latest = await sources.list();
      showBadge(latest.attention);
      if (!panel.hidden) panel.innerHTML = notificationPanelMarkup(latest);
    } catch {
      /* Keep the previous state; the next refresh retries. */
    }
  };
  const close = () => { panel.hidden = true; toggle.setAttribute("aria-expanded", "false"); };
  toggle.addEventListener("click", () => {
    if (!panel.hidden) { close(); return; }
    panel.hidden = false;
    toggle.setAttribute("aria-expanded", "true");
    panel.innerHTML = latest ? notificationPanelMarkup(latest) : '<p class="notification-empty">Wird geladen …</p>';
    void sources.markSeen().then(load, () => undefined);
  });
  panel.addEventListener("click", (event) => {
    const target = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-notification-view]");
    if (!target) return;
    close();
    open(target.dataset.notificationView as AppNotification["view"], target.dataset.notificationModel);
  });
  const escape = (event: KeyboardEvent) => { if (event.key === "Escape" && !panel.hidden) { close(); toggle.focus(); } };
  win.document.addEventListener("keydown", escape);
  void load();
  const timer = win.setInterval(() => void load(), NOTIFICATION_REFRESH_MS);
  return () => { win.clearInterval(timer); win.document.removeEventListener("keydown", escape); };
}

function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
