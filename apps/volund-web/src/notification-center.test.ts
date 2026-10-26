// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  NOTIFICATION_REFRESH_MS, mountNotifications, notificationItemMarkup, notificationPanelMarkup, notificationToggleMarkup,
  type AppNotification, type NotificationPage,
} from "./notification-center";

const failure: AppNotification = {
  key: "conversion:1", category: "job", kind: "event", severity: "warning", title: "Konvertierung fehlgeschlagen",
  message: "parts/<frame>.step", occurredAtUnixMs: 1, view: "model-problems", modelId: "model-1", unread: true,
};
const backup: AppNotification = {
  ...failure, key: "backup:backup", category: "backup", kind: "condition", severity: "error",
  title: "Backup veraltet", message: "backup", view: "administration", modelId: null,
};

function page(items: AppNotification[], attention = items.filter((item) => item.unread).length): NotificationPage {
  return { items, attention, seenAtUnixMs: null };
}

function mount(sources: { list: () => Promise<NotificationPage>; markSeen: () => Promise<void> }) {
  const host = document.createElement("div");
  host.innerHTML = notificationToggleMarkup();
  document.body.append(host);
  const toggle = host.querySelector<HTMLButtonElement>("#notifications-toggle")!;
  const panel = host.querySelector<HTMLElement>("#notification-panel")!;
  const open = vi.fn();
  const dispose = mountNotifications(toggle, panel, sources, open);
  return { toggle, panel, open, dispose, badge: host.querySelector<HTMLElement>("#notification-count")! };
}

afterEach(() => { vi.useRealTimers(); document.body.replaceChildren(); });

describe("notification center", () => {
  it("renders escaped entries with their state", () => {
    expect(notificationItemMarkup(failure)).toContain("parts/&lt;frame&gt;.step");
    expect(notificationItemMarkup(failure)).toContain('data-notification-model="model-1"');
    expect(notificationItemMarkup(backup)).toContain("besteht weiter");
    expect(notificationItemMarkup({ ...failure, unread: false })).toContain("gelesen");
    expect(notificationPanelMarkup(page([]))).toContain("Alles in Ordnung");
  });

  it("shows the attention badge, marks events seen on open, and navigates", async () => {
    const list = vi.fn()
      .mockResolvedValueOnce(page([failure, backup]))
      .mockResolvedValue(page([{ ...failure, unread: false }, backup]));
    const markSeen = vi.fn().mockResolvedValue(undefined);
    const { toggle, panel, open, badge, dispose } = mount({ list, markSeen });
    await vi.waitFor(() => expect(badge.textContent).toBe("2"));
    expect(badge.hidden).toBe(false);
    expect(toggle.getAttribute("aria-label")).toContain("2 erfordern Aufmerksamkeit");
    toggle.click();
    expect(panel.hidden).toBe(false);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    await vi.waitFor(() => expect(markSeen).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(badge.textContent).toBe("1"), { timeout: 2000 });
    panel.querySelector<HTMLButtonElement>('[data-notification-model="model-1"]')!.click();
    expect(open).toHaveBeenCalledWith("model-problems", "model-1");
    expect(panel.hidden).toBe(true);
    dispose();
  });

  it("closes with Escape, hides an empty badge, and refreshes periodically", async () => {
    vi.useFakeTimers();
    const list = vi.fn().mockResolvedValue(page([]));
    const { toggle, panel, badge, dispose } = mount({ list, markSeen: vi.fn().mockResolvedValue(undefined) });
    await vi.advanceTimersByTimeAsync(0);
    expect(badge.hidden).toBe(true);
    toggle.click();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(panel.hidden).toBe(true);
    const calls = list.mock.calls.length;
    await vi.advanceTimersByTimeAsync(NOTIFICATION_REFRESH_MS);
    expect(list.mock.calls.length).toBeGreaterThan(calls);
    dispose();
    const afterDispose = list.mock.calls.length;
    await vi.advanceTimersByTimeAsync(NOTIFICATION_REFRESH_MS);
    expect(list.mock.calls.length).toBe(afterDispose);
  });
});
