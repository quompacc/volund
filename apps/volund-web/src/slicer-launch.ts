import { slicerHandoffStatusMarkup, slicerNoReactionMarkup } from "./model-presentation";
import type { SlicerHandoff, SlicerTarget } from "./types";

/** Configured slicer targets, or `null` when the server reports the handoff as not usable. */
export type SlicerTargets = SlicerTarget[] | null;

/** How long the page waits for the browser to hand the link to a local program. */
export const SLICER_REACTION_MS = 2500;

/** Load targets; any failure means "unavailable", never "no targets configured". */
export async function loadSlicerTargets(load: () => Promise<SlicerTarget[]>): Promise<SlicerTargets> {
  try {
    return await load();
  } catch {
    return null;
  }
}

/**
 * Call `onNoReaction` when the page keeps focus and visibility after a launch:
 * a registered slicer takes focus or hides the page, a missing one leaves it untouched.
 */
export function watchSlicerLaunch(win: Window, onNoReaction: () => void, delayMs = SLICER_REACTION_MS): () => void {
  let reacted = false;
  const react = () => { reacted = true; };
  const onVisibility = () => { if (win.document.visibilityState === "hidden") react(); };
  win.addEventListener("blur", react);
  win.document.addEventListener("visibilitychange", onVisibility);
  const stop = () => {
    win.clearTimeout(timer);
    win.removeEventListener("blur", react);
    win.document.removeEventListener("visibilitychange", onVisibility);
  };
  const timer = win.setTimeout(() => {
    stop();
    if (!reacted && win.document.hasFocus()) onNoReaction();
  }, delayMs);
  return stop;
}

/** Wire the slicer buttons of one file dialog. */
export function mountSlicerButtons(
  dialog: HTMLElement,
  createHandoff: (targetId: string) => Promise<SlicerHandoff>,
  win: Window = window,
): void {
  dialog.querySelectorAll<HTMLButtonElement>("[data-slicer-target]").forEach((button) => button.addEventListener("click", async () => {
    const status = dialog.querySelector<HTMLElement>("#slicer-status")!;
    button.disabled = true;
    status.textContent = "Slicer-Übergabe wird vorbereitet …";
    try {
      const handoff = await createHandoff(button.dataset.slicerTarget!);
      status.innerHTML = slicerHandoffStatusMarkup(handoff.downloadUrl);
      watchSlicerLaunch(win, () => { status.innerHTML = slicerNoReactionMarkup(handoff.downloadUrl); });
      win.location.href = handoff.launchUrl;
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : "Slicer konnte nicht geöffnet werden.";
    } finally {
      button.disabled = false;
    }
  }));
}
