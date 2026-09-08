import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";

export interface ViewerPreferences {
  background: "dark" | "light" | "system";
  gridVisible: boolean;
  contrast: "balanced" | "high";
  renderStyle: "solid" | "wireframe";
}

export interface InspectionLightPositions {
  key: [number, number, number];
  fill: [number, number, number];
  rim: [number, number, number];
}

export interface InspectionGridPlacement {
  centerX: number;
  centerZ: number;
  floorY: number;
  size: number;
}

export interface ViewerResourceSnapshot {
  geometries: number;
  textures: number;
  programs: number;
  renderCalls: number;
  triangles: number;
  modelMeshes: number;
}

export function inspectionGridPlacement(
  minimum: [number, number, number],
  maximum: [number, number, number],
): InspectionGridPlacement {
  const sizeX = Math.max(maximum[0] - minimum[0], 0);
  const sizeY = Math.max(maximum[1] - minimum[1], 0);
  const sizeZ = Math.max(maximum[2] - minimum[2], 0);
  const footprint = Math.max(sizeX, sizeZ, 0.01);
  return {
    centerX: (minimum[0] + maximum[0]) / 2,
    centerZ: (minimum[2] + maximum[2]) / 2,
    floorY: minimum[1] - Math.max(sizeY * 0.02, footprint * 0.005, 0.001),
    size: footprint * 1.5,
  };
}

export function viewerObjectName(name: string): string {
  return THREE.PropertyBinding.sanitizeNodeName(name);
}

export function inspectionLightPositions(
  cameraPosition: [number, number, number],
  targetPosition: [number, number, number],
  cameraUp: [number, number, number] = [0, 1, 0],
): InspectionLightPositions {
  const target = new THREE.Vector3(...targetPosition);
  const view = target.clone().sub(new THREE.Vector3(...cameraPosition)).normalize();
  const up = new THREE.Vector3(...cameraUp).normalize();
  let right = view.clone().cross(up).normalize();
  if (right.lengthSq() < 0.5) right = new THREE.Vector3(1, 0, 0);
  const resolvedUp = right.clone().cross(view).normalize();
  const point = (rightOffset: number, upOffset: number, viewOffset: number): [number, number, number] =>
    target.clone().addScaledVector(right, rightOffset).addScaledVector(resolvedUp, upOffset).addScaledVector(view, viewOffset).toArray() as [number, number, number];
  return {
    key: point(4, 5, -4),
    fill: point(-4, 2, -2),
    rim: point(0, 3, 5),
  };
}

export function restoreViewerStage(
  home: HTMLElement,
  stage: HTMLElement,
  refresh: () => void,
  schedule: (callback: FrameRequestCallback) => number = requestAnimationFrame,
): void {
  home.append(stage);
  schedule(() => refresh());
}

export class CadViewer {
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(38, 1, 0.01, 100_000);
  private readonly renderer: THREE.WebGLRenderer;
  private readonly controls: OrbitControls;
  private readonly loader = new GLTFLoader();
  private readonly stlLoader = new STLLoader();
  private readonly keyLight = new THREE.DirectionalLight(0xfff4d6, 3.2);
  private readonly fillLight = new THREE.DirectionalLight(0xc7d2c0, 1.35);
  private readonly rimLight = new THREE.DirectionalLight(0x91a77f, 1.8);
  private readonly lightTarget = new THREE.Object3D();
  private readonly grid?: THREE.GridHelper;
  private model?: THREE.Object3D;
  private selectionHelper?: THREE.BoxHelper;
  private selectedObject?: THREE.Object3D;
  private readonly resizeObserver?: ResizeObserver;
  private frame = 0;
  private loadGeneration = 0;
  private disposed = false;

  constructor(private readonly canvas: HTMLCanvasElement, private readonly preferences?: ViewerPreferences) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = preferences?.contrast === "high" ? 1.35 : 1.15;
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.08;
    this.controls.screenSpacePanning = true;
    this.scene.add(new THREE.HemisphereLight(0xe8f0dc, 0x2b3027, 2.6));
    this.keyLight.target = this.lightTarget;
    this.fillLight.target = this.lightTarget;
    this.rimLight.target = this.lightTarget;
    this.scene.add(this.keyLight, this.fillLight, this.rimLight, this.lightTarget);
    const lightBackground = preferences?.background === "light"
      || preferences?.background === "system" && window.matchMedia("(prefers-color-scheme: light)").matches;
    this.scene.background = new THREE.Color(lightBackground ? 0xf3f4f6 : 0x111827);
    if (preferences?.gridVisible !== false) {
      this.grid = new THREE.GridHelper(10, 20, lightBackground ? 0x6b7280 : 0x9ca3af, lightBackground ? 0xd1d5db : 0x374151);
      this.grid.name = "inspection-grid";
      this.scene.add(this.grid);
    }
    this.camera.position.set(4, 3, 5);
    this.animate();
    window.addEventListener("resize", this.refreshLayout);
    if (typeof ResizeObserver !== "undefined") {
      this.resizeObserver = new ResizeObserver(() => this.refreshLayout());
      this.resizeObserver.observe(canvas);
    }
    this.refreshLayout();
  }

  async load(url: string, format: "glb" | "stl" = "glb", rotation: [number, number, number] = [0, 0, 0]): Promise<void> {
    if (this.disposed) return;
    const generation = ++this.loadGeneration;
    this.clearModel();
    if (format === "stl") {
      const geometry = await this.stlLoader.loadAsync(url);
      if (!this.isCurrentLoad(generation)) {
        geometry.dispose();
        return;
      }
      geometry.computeVertexNormals();
      const mesh = new THREE.Mesh(
        geometry,
        new THREE.MeshStandardMaterial({ color: 0xd7d4c7, metalness: 0.08, roughness: 0.72 }),
      );
      mesh.rotation.x = -Math.PI / 2;
      this.model = mesh;
    } else {
      const gltf = await this.loader.loadAsync(url);
      if (!this.isCurrentLoad(generation)) {
        disposeObjectResources(gltf.scene);
        return;
      }
      this.model = gltf.scene;
    }
    if (this.preferences?.renderStyle === "wireframe") {
      this.model.traverse((object) => {
        if (!(object instanceof THREE.Mesh)) return;
        const materials = Array.isArray(object.material) ? object.material : [object.material];
        materials.forEach((material) => {
          if (material instanceof THREE.MeshStandardMaterial || material instanceof THREE.MeshBasicMaterial) material.wireframe = true;
        });
      });
    }
    this.applyRotation(rotation, format === "stl");
    this.scene.add(this.model);
    this.frameModel(this.model);
  }

  private applyRotation(rotation: [number, number, number], stl: boolean): void {
    if (!this.model) return;
    this.model.rotation.set(
      THREE.MathUtils.degToRad(rotation[0]) - (stl ? Math.PI / 2 : 0),
      THREE.MathUtils.degToRad(rotation[1]),
      THREE.MathUtils.degToRad(rotation[2]),
    );
  }

  clear(): void {
    this.loadGeneration += 1;
    this.clearModel();
  }

  private clearModel(): void {
    this.clearSelection();
    if (!this.model) return;
    this.scene.remove(this.model);
    disposeObjectResources(this.model);
    this.model = undefined;
  }

  selectObject(name: string): void {
    this.clearSelection();
    const object = this.findObject(name);
    if (!object) return;
    this.selectedObject = object;
    this.selectionHelper = new THREE.BoxHelper(object, 0xf4b84a);
    this.selectionHelper.name = "inspection-selection";
    this.scene.add(this.selectionHelper);
  }

  setObjectVisible(name: string, visible: boolean): void {
    const expected = viewerObjectName(name);
    let hidesSelection = false;
    this.model?.traverse((object) => {
      if (object.name !== name && object.name !== expected) return;
      object.visible = visible;
      hidesSelection ||= !visible && this.selectedObject !== undefined
        && (object === this.selectedObject || isAncestor(object, this.selectedObject));
    });
    if (hidesSelection) this.clearSelection();
  }

  isolateObject(name: string): void {
    const target = this.findObject(name);
    if (!target || !this.model) return;
    this.model.traverse((object) => { object.visible = object === target || isAncestor(object, target) || isAncestor(target, object); });
    this.selectObject(name);
  }

  resetVisibility(): void {
    this.model?.traverse((object) => { object.visible = true; });
    this.clearSelection();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    cancelAnimationFrame(this.frame);
    window.removeEventListener("resize", this.refreshLayout);
    this.resizeObserver?.disconnect();
    this.clear();
    this.controls.dispose();
    this.renderer.dispose();
  }

  private isCurrentLoad(generation: number): boolean {
    return !this.disposed && generation === this.loadGeneration;
  }

  readonly refreshLayout = (): void => {
    const width = this.canvas.clientWidth;
    const height = this.canvas.clientHeight;
    if (width === 0 || height === 0) return;
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(width, height, false);
  };

  resourceSnapshot(): ViewerResourceSnapshot {
    let modelMeshes = 0;
    this.model?.traverse((object) => { if (object instanceof THREE.Mesh) modelMeshes += 1; });
    return {
      geometries: this.renderer.info.memory.geometries,
      textures: this.renderer.info.memory.textures,
      programs: this.renderer.info.programs?.length ?? 0,
      renderCalls: this.renderer.info.render.calls,
      triangles: this.renderer.info.render.triangles,
      modelMeshes,
    };
  }

  private frameModel(model: THREE.Object3D): void {
    const bounds = new THREE.Box3().setFromObject(model);
    const size = bounds.getSize(new THREE.Vector3());
    const center = bounds.getCenter(new THREE.Vector3());
    const radius = Math.max(size.length() / 2, 0.01);
    if (this.grid) {
      const placement = inspectionGridPlacement(
        bounds.min.toArray() as [number, number, number],
        bounds.max.toArray() as [number, number, number],
      );
      this.grid.position.set(placement.centerX, placement.floorY, placement.centerZ);
      this.grid.scale.set(placement.size / 10, 1, placement.size / 10);
    }
    const distance = radius / Math.sin(THREE.MathUtils.degToRad(this.camera.fov / 2));
    this.controls.target.copy(center);
    this.camera.position.copy(center).add(new THREE.Vector3(0.8, 0.55, 1).normalize().multiplyScalar(distance));
    this.camera.near = Math.max(distance / 10_000, 0.001);
    this.camera.far = distance * 20;
    this.camera.updateProjectionMatrix();
    this.controls.update();
  }

  private readonly animate = (): void => {
    this.frame = requestAnimationFrame(this.animate);
    this.controls.update();
    this.updateInspectionLights();
    this.renderer.render(this.scene, this.camera);
  };

  private updateInspectionLights(): void {
    const target = this.controls.target;
    const positions = inspectionLightPositions(
      this.camera.position.toArray() as [number, number, number],
      target.toArray() as [number, number, number],
      this.camera.up.toArray() as [number, number, number],
    );
    this.lightTarget.position.copy(target);
    this.keyLight.position.set(...positions.key);
    this.fillLight.position.set(...positions.fill);
    this.rimLight.position.set(...positions.rim);
  }

  private findObject(name: string): THREE.Object3D | undefined {
    let match: THREE.Object3D | undefined;
    const expected = viewerObjectName(name);
    this.model?.traverse((object) => { if (!match && (object.name === name || object.name === expected)) match = object; });
    return match;
  }

  private clearSelection(): void {
    this.selectedObject = undefined;
    if (!this.selectionHelper) return;
    this.scene.remove(this.selectionHelper);
    this.selectionHelper.geometry.dispose();
    this.selectionHelper.material.dispose();
    this.selectionHelper = undefined;
  }
}

function isAncestor(candidate: THREE.Object3D, object: THREE.Object3D): boolean {
  for (let parent = object.parent; parent; parent = parent.parent) if (parent === candidate) return true;
  return false;
}

function disposeObjectResources(root: THREE.Object3D): void {
  const geometries = new Set<THREE.BufferGeometry>();
  const materials = new Set<THREE.Material>();
  const textures = new Set<THREE.Texture>();
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    geometries.add(object.geometry);
    const meshMaterials = Array.isArray(object.material) ? object.material : [object.material];
    meshMaterials.forEach((material) => {
      materials.add(material);
      Object.values(material).forEach((value) => {
        if (value instanceof THREE.Texture) textures.add(value);
      });
    });
  });
  textures.forEach((texture) => texture.dispose());
  materials.forEach((material) => material.dispose());
  geometries.forEach((geometry) => geometry.dispose());
}
