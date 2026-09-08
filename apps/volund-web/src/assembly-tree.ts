export interface AssemblyDefinition {
  id: string; name: string; kind: "assembly" | "part";
  color: [number, number, number] | null; properties?: Record<string, string>;
}
export interface AssemblyNode {
  id: string; name: string; kind: "assembly" | "assembly-instance" | "part" | "part-instance";
  definition: string; transform: number[]; color: [number, number, number] | null;
  properties?: Record<string, string>; children: AssemblyNode[];
}
export interface AssemblyManifest {
  contractVersion: number; transformConvention: string; colorSpace: string;
  definitions: AssemblyDefinition[]; roots: AssemblyNode[];
}
export interface AssemblyViewerActions {
  select(name: string): void; visible(name: string, visible: boolean): void;
  isolate(name: string): void; reset(): void;
}
interface AssemblyCounts { assemblies: number; parts: number; instances: number; total: number }
const noSelectionMarkup = "<h3>Keine Auswahl</h3><p>Wähle einen Knoten, um Definition, Farbe, Transformation und Eigenschaften textuell zu prüfen.</p>";

export function parseAssemblyManifest(value: unknown): AssemblyManifest {
  if (!isRecord(value) || value.contractVersion !== 1 || value.transformConvention !== "row-major, parent-local"
      || value.colorSpace !== "sRGB" || !Array.isArray(value.definitions) || !Array.isArray(value.roots)
      || value.definitions.length > 20_000) throw new Error("Das Strukturmanifest ist ungültig oder nicht unterstützt.");
  const definitions = new Set<string>();
  for (const definition of value.definitions) {
    if (!isDefinition(definition) || definitions.has(definition.id)) throw new Error("Das Strukturmanifest enthält eine ungültige Definition.");
    definitions.add(definition.id);
  }
  const pending: Array<{ value: unknown; depth: number }> = value.roots.map((root) => ({ value: root, depth: 0 }));
  const nodes = new Set<string>(); let count = 0;
  while (pending.length > 0) {
    const current = pending.pop()!;
    if (current.depth > 256) throw new Error("Die Struktur überschreitet 256 Ebenen.");
    if (!isAssemblyNode(current.value) || nodes.has(current.value.id) || !definitions.has(current.value.definition)) {
      throw new Error("Das Strukturmanifest enthält einen ungültigen Knoten.");
    }
    nodes.add(current.value.id); count += 1;
    if (count > 20_000) throw new Error("Die Struktur überschreitet 20.000 Knoten.");
    current.value.children.forEach((child) => pending.push({ value: child, depth: current.depth + 1 }));
  }
  return value as unknown as AssemblyManifest;
}

export function assemblyTreeMarkup(manifest: AssemblyManifest, primaryName: string): string {
  const counts = countNodes(manifest.roots);
  const roots = manifest.roots.map((node) => nodeMarkup(node, 0)).join("");
  return `<div class="assembly-source"><span>PRIMÄRE CAD- ODER MESH-DATEI</span><strong>${escapeMarkup(primaryName)}</strong><small>${counts.assemblies} Baugruppen · ${counts.parts} Teile · ${counts.instances} Instanzen · ${counts.total} Knoten</small></div>
    <div class="assembly-toolbar"><button type="button" data-assembly-reset>Alle einblenden</button><span>Sichtbarkeit ist nur Ansichtszeit und verändert keine CAD-Datei.</span></div>
    <div class="assembly-inspector"><div class="assembly-tree" role="tree" aria-label="Baugruppenstruktur">${roots || '<p class="loading-copy">Die primäre Datei enthält keine Produktstruktur.</p>'}</div><aside id="assembly-selected" aria-live="polite">${noSelectionMarkup}</aside></div>`;
}

export function mountAssemblyTree(host: HTMLElement, manifest: AssemblyManifest, actions?: AssemblyViewerActions): void {
  const nodes = new Map<string, AssemblyNode>(); const pending = [...manifest.roots];
  while (pending.length > 0) { const node = pending.pop()!; nodes.set(node.id, node); pending.push(...node.children); }
  const select = (item: HTMLElement): void => {
    host.querySelectorAll("[data-assembly-id]").forEach((entry) => entry.classList.remove("selected")); item.classList.add("selected");
    const node = nodes.get(item.dataset.assemblyId!); if (!node) return;
    host.querySelector<HTMLElement>("#assembly-selected")!.innerHTML = selectedMarkup(node, manifest.definitions.find((entry) => entry.id === node.definition));
    actions?.select(node.name);
  };
  host.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const action = target.closest<HTMLButtonElement>("[data-assembly-action]");
    const item = target.closest<HTMLElement>("[data-assembly-id]");
    if (action && item) {
      event.preventDefault(); event.stopPropagation(); const name = nodes.get(item.dataset.assemblyId!)?.name; if (!name) return;
      if (action.dataset.assemblyAction === "visibility") {
        const hidden = !item.classList.contains("assembly-hidden");
        const affected = visibilityItems(item, hidden);
        affected.forEach((entry) => {
          setVisibilityMarkup(entry, hidden); entry.classList.remove("isolated-away");
          const entryName = nodes.get(entry.dataset.assemblyId!)?.name;
          if (entryName) actions?.visible(entryName, !hidden);
        });
        if (hidden && selectedWithin(host, item)) clearTreeSelection(host);
      } else {
        host.querySelectorAll<HTMLElement>("[data-assembly-id]").forEach((entry) => {
          const hidden = entry !== item && !entry.contains(item) && !item.contains(entry);
          entry.classList.toggle("isolated-away", hidden); setVisibilityMarkup(entry, hidden);
        });
        select(item); actions?.isolate(name);
      }
      return;
    }
    if (item) select(item);
  });
  host.querySelector("[data-assembly-reset]")?.addEventListener("click", () => {
    host.querySelectorAll<HTMLElement>("[data-assembly-id]").forEach((item) => {
      item.classList.remove("assembly-hidden", "isolated-away", "selected");
      setVisibilityMarkup(item, false);
    });
    clearTreeSelection(host);
    actions?.reset();
  });
  host.querySelector(".assembly-tree")?.addEventListener("keydown", (event) => {
    if (!(event instanceof KeyboardEvent) || !["ArrowDown", "ArrowUp"].includes(event.key)) return;
    const items = [...host.querySelectorAll<HTMLElement>('[role="treeitem"]')];
    const current = (event.target as HTMLElement).closest<HTMLElement>('[role="treeitem"]');
    const index = Math.max(items.indexOf(current!), 0);
    items[(index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus(); event.preventDefault();
  });
}

function visibilityItems(item: HTMLElement, hidden: boolean): HTMLElement[] {
  const result = [item, ...item.querySelectorAll<HTMLElement>("[data-assembly-id]")];
  if (hidden) return result;
  for (let parent = item.parentElement?.closest<HTMLElement>("[data-assembly-id]"); parent;
    parent = parent.parentElement?.closest<HTMLElement>("[data-assembly-id]")) result.push(parent);
  return [...new Set(result)];
}

function selectedWithin(host: HTMLElement, item: HTMLElement): boolean {
  const selected = host.querySelector<HTMLElement>("[data-assembly-id].selected");
  return selected != null && (selected === item || item.contains(selected));
}

function clearTreeSelection(host: HTMLElement): void {
  host.querySelectorAll("[data-assembly-id].selected").forEach((entry) => entry.classList.remove("selected"));
  host.querySelector<HTMLElement>("#assembly-selected")!.innerHTML = noSelectionMarkup;
}

function setVisibilityMarkup(item: HTMLElement, hidden: boolean): void {
  item.classList.toggle("assembly-hidden", hidden);
  const button = item.querySelector<HTMLButtonElement>("[data-assembly-action=visibility]");
  if (button) { button.setAttribute("aria-pressed", String(hidden)); button.textContent = hidden ? "Einblenden" : "Ausblenden"; }
}

function countNodes(roots: AssemblyNode[]): AssemblyCounts {
  const counts: AssemblyCounts = { assemblies: 0, parts: 0, instances: 0, total: 0 }; const pending = [...roots];
  while (pending.length > 0) { const node = pending.pop()!; counts.total += 1; if (node.kind.startsWith("assembly")) counts.assemblies += 1; else counts.parts += 1; if (node.kind.endsWith("instance")) counts.instances += 1; pending.push(...node.children); }
  return counts;
}
function nodeMarkup(node: AssemblyNode, depth: number): string {
  const label = node.kind.startsWith("assembly") ? "BAUGRUPPE" : "TEIL"; const instance = node.kind.endsWith("instance") ? " · INSTANZ" : "";
  const line = `<span class="assembly-node-line"><span class="assembly-node-copy"><strong>${escapeMarkup(node.name)}</strong><small>${label}${instance}</small></span><span class="assembly-node-actions"><button type="button" data-assembly-action="visibility" aria-pressed="false">Ausblenden</button><button type="button" data-assembly-action="isolate">Isolieren</button></span></span>`;
  if (node.children.length === 0) return `<div class="assembly-node assembly-leaf" role="treeitem" tabindex="0" data-assembly-id="${escapeMarkup(node.id)}">${line}</div>`;
  return `<details class="assembly-node"${depth === 0 ? " open" : ""} data-assembly-id="${escapeMarkup(node.id)}"><summary role="treeitem" tabindex="0">${line}<em>${node.children.length}</em></summary><div role="group">${node.children.map((child) => nodeMarkup(child, depth + 1)).join("")}</div></details>`;
}
function selectedMarkup(node: AssemblyNode, definition?: AssemblyDefinition): string {
  const color = node.color ?? definition?.color; const properties = { ...(definition?.properties ?? {}), ...(node.properties ?? {}) };
  const rows = Object.entries(properties).map(([key, value]) => `<div><dt>${escapeMarkup(key)}</dt><dd>${escapeMarkup(value)}</dd></div>`).join("");
  return `<h3>${escapeMarkup(node.name)}</h3><dl><div><dt>Stabile Knoten-ID</dt><dd>${escapeMarkup(node.id)}</dd></div><div><dt>Definition</dt><dd>${escapeMarkup(definition?.name ?? node.definition)}</dd></div><div><dt>Farbe (sRGB)</dt><dd>${color ? color.map((value) => value.toFixed(3)).join(" · ") : "Nicht angegeben"}</dd></div><div><dt>Transformation</dt><dd>${node.transform.map((value) => Number(value.toFixed(4))).join(" · ")}</dd></div>${rows}</dl>`;
}
function isDefinition(value: unknown): value is AssemblyDefinition {
  return isRecord(value) && boundedString(value.id, 160) && boundedString(value.name, 500) && ["assembly", "part"].includes(String(value.kind)) && isColor(value.color) && isProperties(value.properties);
}
function isAssemblyNode(value: unknown): value is AssemblyNode {
  return isRecord(value) && boundedString(value.id, 160) && boundedString(value.name, 500) && boundedString(value.definition, 160)
    && ["assembly", "assembly-instance", "part", "part-instance"].includes(String(value.kind))
    && Array.isArray(value.transform) && value.transform.length === 16 && value.transform.every((entry) => typeof entry === "number" && Number.isFinite(entry) && Math.abs(entry) <= 1e12)
    && isColor(value.color) && isProperties(value.properties) && Array.isArray(value.children);
}
function isColor(value: unknown): value is [number, number, number] | null { return value === null || Array.isArray(value) && value.length === 3 && value.every((entry) => typeof entry === "number" && Number.isFinite(entry) && entry >= 0 && entry <= 1) }
function isProperties(value: unknown): boolean { return value === undefined || isRecord(value) && Object.keys(value).length <= 32 && Object.entries(value).every(([key, item]) => key.length <= 160 && typeof item === "string" && item.length <= 1000) }
function boundedString(value: unknown, limit: number): value is string { return typeof value === "string" && value.length > 0 && value.length <= limit }
function isRecord(value: unknown): value is Record<string, unknown> { return typeof value === "object" && value !== null }
function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;") }
