interface TextInputRequest {
  title: string;
  label: string;
  initialValue: string;
  submitLabel: string;
  maxLength?: number;
}

export function requestTextInput(request: TextInputRequest): Promise<string | null> {
  if (document.querySelector("[data-text-input-dialog]")) {
    return Promise.reject(new Error("Bitte zuerst den offenen Dialog abschließen."));
  }
  const previousFocus = document.activeElement as HTMLElement | null;
  const dialog = document.createElement("dialog");
  dialog.dataset.textInputDialog = "";
  dialog.className = "exact-confirmation";
  dialog.setAttribute("aria-label", request.title);
  dialog.innerHTML = `<form><h2></h2><label><span></span><input required></label><div class="confirmation-actions"><button type="button" class="secondary-action">Abbrechen</button><button type="submit" class="primary-action"></button></div></form>`;
  dialog.querySelector("h2")!.textContent = request.title;
  dialog.querySelector("label span")!.textContent = request.label;
  const input = dialog.querySelector<HTMLInputElement>("input")!;
  input.value = request.initialValue;
  if (request.maxLength) input.maxLength = request.maxLength;
  dialog.querySelector<HTMLButtonElement>("[type='submit']")!.textContent = request.submitLabel;
  return new Promise((resolve) => {
    let finished = false;
    const finish = (value: string | null): void => {
      if (finished) return;
      finished = true;
      if (dialog.open && typeof dialog.close === "function") dialog.close();
      dialog.remove();
      if (previousFocus?.isConnected) previousFocus.focus();
      resolve(value);
    };
    dialog.querySelector("form")!.addEventListener("submit", (event) => {
      event.preventDefault();
      const value = input.value.trim();
      if (value) finish(value);
    });
    dialog.querySelector("[type='button']")!.addEventListener("click", () => finish(null));
    dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish(null); });
    document.body.append(dialog);
    if (typeof dialog.showModal === "function") dialog.showModal();
    else dialog.setAttribute("open", "");
    input.focus();
  });
}
