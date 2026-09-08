/** Explicit, keyboard-accessible confirmation without browser-native prompts. */
export function confirmExact(expected: string, effect: string): Promise<string> {
  if (document.querySelector("[data-exact-confirmation]")) {
    return Promise.reject(new Error("Bitte zuerst die offene Bestätigung abschließen."));
  }
  const previousFocus = document.activeElement as HTMLElement | null;
  const dialog = document.createElement("dialog");
  dialog.dataset.exactConfirmation = "";
  dialog.className = "exact-confirmation";
  dialog.setAttribute("aria-label", "Aktion bestätigen");
  dialog.innerHTML = `<form><h2>Aktion bestätigen</h2><p data-effect></p>
    <label>Zur Bestätigung exakt eingeben:<strong data-expected></strong>
    <input name="confirmation" autocomplete="off" spellcheck="false" required></label>
    <div class="confirmation-actions"><button type="button" class="secondary-action">Abbrechen</button>
    <button type="submit" class="primary-action" disabled>Bestätigen</button></div></form>`;
  dialog.querySelector("[data-effect]")!.textContent = effect;
  dialog.querySelector("[data-expected]")!.textContent = expected;
  const form = dialog.querySelector("form")!;
  const input = dialog.querySelector("input")!;
  const submit = dialog.querySelector<HTMLButtonElement>("[type=submit]")!;
  return new Promise((resolve, reject) => {
    let finished = false;
    const finish = (confirmed: boolean): void => {
      if (finished) return;
      finished = true;
      dialog.remove();
      if (previousFocus?.isConnected) previousFocus.focus();
      if (confirmed) resolve(expected);
      else reject(new Error("Aktion abgebrochen."));
    };
    input.addEventListener("input", () => { submit.disabled = input.value !== expected; });
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (input.value === expected) finish(true);
    });
    dialog.querySelector("[type=button]")!.addEventListener("click", () => finish(false));
    dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish(false); });
    dialog.addEventListener("close", () => finish(false));
    document.body.append(dialog);
    try { dialog.showModal(); input.focus(); }
    catch { finish(false); }
  });
}
