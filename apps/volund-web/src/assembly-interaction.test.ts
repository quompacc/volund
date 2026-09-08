// @vitest-environment happy-dom

import { afterEach, describe, expect, it } from "vitest";
import * as THREE from "three";
import { assemblyTreeMarkup, mountAssemblyTree } from "./assembly-tree";
import { CadViewer } from "./viewer";
import { manifest } from "../tests/assembly-fixture";

afterEach(() => document.body.replaceChildren());

function setup() {
  const model = new THREE.Group();
  model.name = "Baugruppe";
  const subgroup = new THREE.Group();
  subgroup.name = "Unterbaugruppe";
  model.add(subgroup);
  for (const name of ["Rot", "Blau"]) {
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshBasicMaterial());
    mesh.name = name;
    subgroup.add(mesh);
  }
  const viewer: CadViewer = Object.assign(Object.create(CadViewer.prototype), {
    model, scene: new THREE.Scene(),
  });
  const host = document.createElement("main");
  host.innerHTML = assemblyTreeMarkup(manifest, "synthetisch.step");
  document.body.append(host);
  mountAssemblyTree(host, manifest, {
    select: (name) => viewer.selectObject(name),
    visible: (name, visible) => viewer.setObjectVisible(name, visible),
    isolate: (name) => viewer.isolateObject(name), reset: () => viewer.resetVisibility(),
  });
  const row = (id: string) => host.querySelector<HTMLElement>(`[data-assembly-id="${id}"]`)!;
  const button = (id: string, action: string) => row(id).querySelector<HTMLButtonElement>(`[data-assembly-action="${action}"]`)!;
  return { host, model, viewer, row, button };
}

describe("assembly tree and actual viewer visibility", () => {
  it("synchronizes hide, isolate, show and reset with the scene", () => {
    const { host, model, viewer, row, button } = setup();
    button("red", "visibility").click();
    expect(model.getObjectByName("Rot")!.visible).toBe(false);
    button("red", "isolate").click();
    expect(model.getObjectByName("Rot")!.visible).toBe(true);
    expect(model.getObjectByName("Blau")!.visible).toBe(false);
    expect(button("red", "visibility").textContent).toBe("Ausblenden");
    expect(button("blue", "visibility").textContent).toBe("Einblenden");
    expect(row("red").classList.contains("assembly-hidden")).toBe(false);
    button("blue", "visibility").click();
    expect(model.getObjectByName("Blau")!.visible).toBe(true);
    expect(row("blue").classList.contains("isolated-away")).toBe(false);
    host.querySelector<HTMLButtonElement>("[data-assembly-reset]")!.click();
    expect([...host.querySelectorAll("[aria-pressed=true]")]).toHaveLength(0);
    expect(model.children.every((child) => child.visible)).toBe(true);
    viewer.clear();
  });

  it("selects the isolated node and clears details on reset", () => {
    const { host, viewer, row, button } = setup();
    row("blue").click();
    expect(host.querySelector("#assembly-selected h3")!.textContent).toBe("Blau");
    button("red", "isolate").click();
    expect(host.querySelector("#assembly-selected h3")!.textContent).toBe("Rot");
    expect(row("red").classList.contains("selected")).toBe(true);
    host.querySelector<HTMLButtonElement>("[data-assembly-reset]")!.click();
    expect(host.querySelector("#assembly-selected h3")!.textContent).toBe("Keine Auswahl");
    expect(host.querySelector(".selected")).toBeNull();
    viewer.clear();
  });

  it("clears selected details when all objects are restored", () => {
    const { host, viewer, row } = setup();
    row("blue").click();
    host.querySelector<HTMLButtonElement>("[data-assembly-reset]")!.click();
    expect(host.querySelector("#assembly-selected h3")!.textContent).toBe("Keine Auswahl");
    viewer.clear();
  });

  it("keeps inherited visibility and selection synchronized for nested assemblies", () => {
    const { host, model, viewer, row, button } = setup();
    row("red").click();
    button("group", "visibility").click();
    expect(model.getObjectByName("Unterbaugruppe")!.visible).toBe(false);
    expect(model.getObjectByName("Rot")!.visible).toBe(false);
    expect(model.getObjectByName("Blau")!.visible).toBe(false);
    expect(button("group", "visibility").textContent).toBe("Einblenden");
    expect(button("red", "visibility").textContent).toBe("Einblenden");
    expect(button("blue", "visibility").textContent).toBe("Einblenden");
    expect(host.querySelector("#assembly-selected h3")!.textContent).toBe("Keine Auswahl");
    expect(host.querySelector(".selected")).toBeNull();

    button("blue", "visibility").click();
    expect(model.getObjectByName("Baugruppe")!.visible).toBe(true);
    expect(model.getObjectByName("Unterbaugruppe")!.visible).toBe(true);
    expect(model.getObjectByName("Rot")!.visible).toBe(false);
    expect(model.getObjectByName("Blau")!.visible).toBe(true);
    expect(button("group", "visibility").textContent).toBe("Ausblenden");
    expect(button("red", "visibility").textContent).toBe("Einblenden");
    expect(button("blue", "visibility").textContent).toBe("Ausblenden");
    viewer.clear();
  });

  it("preserves selection when an unrelated sibling is hidden", () => {
    const { host, viewer, row, button } = setup();
    row("red").click();
    button("blue", "visibility").click();
    expect(row("red").classList.contains("selected")).toBe(true);
    expect(host.querySelector("#assembly-selected h3")!.textContent).toBe("Rot");
    expect((viewer as unknown as { selectionHelper?: THREE.BoxHelper }).selectionHelper).toBeDefined();
    viewer.clear();
  });
});
