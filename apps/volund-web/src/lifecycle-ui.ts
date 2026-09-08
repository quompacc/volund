import { catalogApi } from "./api";
import type { LifecycleAction } from "./types";

export function lifecycleEntry(action: LifecycleAction, targetId: string, revision: number, label: string): string {
  return `<section class="lifecycle-entry" data-lifecycle-action="${escapeMarkup(action)}" data-lifecycle-target="${escapeMarkup(targetId)}" data-lifecycle-revision="${revision}"><button type="button" class="danger-action" data-lifecycle-preview>${escapeMarkup(label)} …</button><div data-lifecycle-state aria-live="polite"></div></section>`;
}

export function bindLifecycleEntries(host: HTMLElement, onApplied: () => void): void {
  host.querySelectorAll<HTMLElement>("[data-lifecycle-action]").forEach((entry) => {
    entry.querySelector<HTMLButtonElement>("[data-lifecycle-preview]")?.addEventListener("click", async (event) => {
      const button = event.currentTarget as HTMLButtonElement;
      const state = entry.querySelector<HTMLElement>("[data-lifecycle-state]")!;
      button.disabled = true;
      state.textContent = "Auswirkungen werden geprüft …";
      try {
        const plan = await catalogApi.previewLifecycle(
          entry.dataset.lifecycleAction as LifecycleAction,
          entry.dataset.lifecycleTarget!,
          Number(entry.dataset.lifecycleRevision),
        );
        state.innerHTML = `<form class="lifecycle-confirmation"><h3>Auswirkungen bestätigen</h3><pre></pre><label>Exakt eingeben: <strong>${escapeMarkup(plan.confirmation)}</strong><input name="confirmation" autocomplete="off" required></label><button class="danger-action">Anwenden</button><p class="form-message" aria-live="polite"></p></form>`;
        state.querySelector("pre")!.textContent = JSON.stringify(plan.impact, null, 2);
        const form = state.querySelector<HTMLFormElement>("form")!;
        form.querySelector<HTMLInputElement>("input")!.focus();
        form.addEventListener("submit", async (submitEvent) => {
          submitEvent.preventDefault();
          const submit = form.querySelector<HTMLButtonElement>("button")!;
          const message = form.querySelector<HTMLElement>(".form-message")!;
          submit.disabled = true; message.textContent = "Aktion wird angewendet …";
          try {
            await catalogApi.applyLifecycle(plan, String(new FormData(form).get("confirmation") || ""));
            message.textContent = "Aktion abgeschlossen."; onApplied();
          } catch (error) {
            submit.disabled = false; message.classList.add("error");
            message.textContent = error instanceof Error ? error.message : "Aktion fehlgeschlagen.";
          }
        });
      } catch (error) {
        button.disabled = false;
        state.textContent = error instanceof Error ? error.message : "Auswirkungen konnten nicht geladen werden.";
      }
    });
  });
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}
