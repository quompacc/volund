export function hideFileContextMenu(
  menu: HTMLElement,
  trigger: HTMLElement | undefined,
  restoreFocus: boolean,
): void {
  menu.hidden = true;
  if (restoreFocus) trigger?.focus();
}
