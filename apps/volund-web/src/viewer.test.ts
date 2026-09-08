import { describe, expect, it, vi } from "vitest";
import * as THREE from "three";
import { CadViewer, inspectionGridPlacement, inspectionLightPositions, restoreViewerStage, viewerObjectName } from "./viewer";

describe("camera-aware inspection lighting", () => {
  it("mirrors the three-point rig when the camera moves behind the model", () => {
    const front = inspectionLightPositions([0, 0, 10], [0, 0, 0]);
    const rear = inspectionLightPositions([0, 0, -10], [0, 0, 0]);

    expect(front.key[2]).toBeGreaterThan(0);
    expect(front.rim[2]).toBeLessThan(0);
    expect(rear.key[2]).toBeLessThan(0);
    expect(rear.rim[2]).toBeGreaterThan(0);
    expect(front.key[1]).toBeCloseTo(rear.key[1]);
    expect(front.fill[1]).toBeCloseTo(rear.fill[1]);
  });

  it("keeps finite stable positions when looking vertically", () => {
    const positions = inspectionLightPositions([0, 10, 0], [0, 0, 0]);
    expect(Object.values(positions).flat().every(Number.isFinite)).toBe(true);
  });
});

describe("inspection grid", () => {
  it("places and sizes the grid below the loaded model", () => {
    const placement = inspectionGridPlacement([-20, -4, 10], [40, 96, 50]);
    expect(placement.centerX).toBe(10);
    expect(placement.centerZ).toBe(30);
    expect(placement.floorY).toBeLessThan(-4);
    expect(placement.size).toBe(90);
  });

  it("keeps a useful floor for flat or tiny geometry", () => {
    const placement = inspectionGridPlacement([0, 0, 0], [0, 0, 0]);
    expect(placement.floorY).toBeLessThan(0);
    expect(placement.size).toBeGreaterThan(0);
  });
});

describe("assembly node names", () => {
  it("uses the same name sanitizing rule as Three.js GLTFLoader", () => {
    expect(viewerObjectName("Gantry:1")).not.toBe("Gantry:1");
    expect(viewerObjectName("_VORON2.4 Assembly v89")).not.toContain(" ");
  });
});

describe("viewer stage layout", () => {
  it("refreshes after the shared stage returns from the file dialog", () => {
    const append = vi.fn();
    const refresh = vi.fn();
    const schedule = vi.fn((callback: FrameRequestCallback) => { callback(0); return 1; });
    restoreViewerStage({ append } as unknown as HTMLElement, {} as HTMLElement, refresh, schedule);
    expect(append).toHaveBeenCalledOnce();
    expect(schedule).toHaveBeenCalledOnce();
    expect(refresh).toHaveBeenCalledOnce();
  });
});

describe("viewer load lifecycle", () => {
  it("discards and releases a GLB that finishes after the viewer was disposed", async () => {
    let finishLoad!: (value: { scene: THREE.Group }) => void;
    const geometry = new THREE.BufferGeometry();
    const material = new THREE.MeshStandardMaterial();
    const disposeGeometry = vi.spyOn(geometry, "dispose");
    const disposeMaterial = vi.spyOn(material, "dispose");
    const model = new THREE.Group();
    model.add(new THREE.Mesh(geometry, material));
    const scene = { add: vi.fn(), remove: vi.fn() };
    const viewer = Object.assign(Object.create(CadViewer.prototype), {
      scene,
      loader: { loadAsync: vi.fn(() => new Promise((resolve) => { finishLoad = resolve; })) },
      loadGeneration: 0,
      disposed: false,
      clear: vi.fn(),
      applyRotation: vi.fn(),
      frameModel: vi.fn(),
    }) as CadViewer;
    const lifecycle = viewer as unknown as { disposed: boolean; loadGeneration: number };

    const loading = viewer.load("/late.glb");
    lifecycle.disposed = true;
    lifecycle.loadGeneration += 1;
    finishLoad({ scene: model });
    await loading;

    expect(scene.add).not.toHaveBeenCalled();
    expect(disposeGeometry).toHaveBeenCalledOnce();
    expect(disposeMaterial).toHaveBeenCalledOnce();
  });

  it("keeps the newest model when an older load finishes last", async () => {
    const resolvers: Array<(value: { scene: THREE.Group }) => void> = [];
    const scene = { add: vi.fn(), remove: vi.fn() };
    const viewer = Object.assign(Object.create(CadViewer.prototype), {
      scene,
      loader: { loadAsync: vi.fn(() => new Promise((resolve) => { resolvers.push(resolve); })) },
      loadGeneration: 0,
      disposed: false,
      clearModel: vi.fn(),
      applyRotation: vi.fn(),
      frameModel: vi.fn(),
    }) as CadViewer;
    const staleGeometry = new THREE.BufferGeometry();
    const staleMaterial = new THREE.MeshStandardMaterial();
    const disposeGeometry = vi.spyOn(staleGeometry, "dispose");
    const disposeMaterial = vi.spyOn(staleMaterial, "dispose");
    const stale = new THREE.Group();
    stale.name = "stale";
    stale.add(new THREE.Mesh(staleGeometry, staleMaterial));
    const current = new THREE.Group();
    current.name = "current";

    const first = viewer.load("/slow.glb");
    const second = viewer.load("/fast.glb");
    resolvers[1]!({ scene: current });
    await second;
    resolvers[0]!({ scene: stale });
    await first;

    expect(scene.add).toHaveBeenCalledOnce();
    expect(scene.add).toHaveBeenCalledWith(current);
    expect(disposeGeometry).toHaveBeenCalledOnce();
    expect(disposeMaterial).toHaveBeenCalledOnce();
  });
});
