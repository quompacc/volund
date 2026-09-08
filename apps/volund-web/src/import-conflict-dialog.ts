export interface ImportConflictDialog {
  element: HTMLDialogElement;
  requestPath(defaultPath: string): Promise<string | null>;
}

export function createImportConflictDialog(): ImportConflictDialog {
  const element = document.createElement("dialog");
  element.id = "import-conflict-dialog";
  element.className = "import-selection-dialog";
  element.innerHTML = `<form><p class="eyebrow">KONFLIKT LÖSEN</p><h2>Zielpfad ändern</h2><label>Neuer relativer Zielpfad<input required></label><small>Der neue Pfad muss innerhalb der ausgewählten Bibliothek liegen.</small><div class="dialog-actions"><button class="secondary-action" type="button">Abbrechen</button><button class="primary-action" type="submit">Ziel übernehmen</button></div></form>`;
  const form = element.querySelector<HTMLFormElement>("form")!;
  const input = element.querySelector<HTMLInputElement>("input")!;
  const cancel = element.querySelector<HTMLButtonElement>("button[type='button']")!;
  let resolveRequest: ((value: string | null) => void) | null = null;
  const finish = (value: string | null): void => {
    if (element.open && typeof element.close === "function") element.close();
    else element.removeAttribute("open");
    resolveRequest?.(value);
    resolveRequest = null;
  };
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const value = input.value.trim();
    if (value) finish(value);
  });
  cancel.addEventListener("click", () => finish(null));
  element.addEventListener("cancel", (event) => {
    event.preventDefault();
    finish(null);
  });
  return {
    element,
    requestPath(defaultPath) {
      if (resolveRequest) throw new Error("import conflict dialog is already open");
      input.value = defaultPath;
      if (typeof element.showModal === "function") element.showModal();
      else element.setAttribute("open", "");
      input.focus();
      return new Promise((resolve) => {
        resolveRequest = resolve;
      });
    },
  };
}
