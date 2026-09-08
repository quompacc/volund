/** Sichtbare Maus- und Tastaturalternative zur horizontalen Wischgeste. */
export function mountNavigationScroll(nav: HTMLElement): void {
  const controls = document.createElement("div");
  controls.className = "nav-scroll-hint";
  controls.innerHTML = '<button type="button" aria-label="Navigation nach links scrollen">←</button><span>Navigation seitlich scrollen</span><button type="button" aria-label="Navigation nach rechts scrollen">→</button>';
  controls.querySelectorAll("button").forEach((button, index) => {
    button.addEventListener("click", () => nav.scrollBy({ left: nav.clientWidth * (index === 0 ? -0.8 : 0.8), behavior: "smooth" }));
  });
  nav.after(controls);
}
