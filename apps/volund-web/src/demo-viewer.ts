import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";

export type DemoPart = "bracket" | "rail";

export class DemoPartViewer {
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(34, 1, 0.01, 500);
  private readonly renderer: THREE.WebGLRenderer;
  private readonly controls: OrbitControls;
  private readonly group = new THREE.Group();
  private frame = 0;

  constructor(private readonly canvas: HTMLCanvasElement, part: DemoPart) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.5));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = true;
    this.controls.enablePan = false;
    this.scene.add(new THREE.HemisphereLight(0xe9e4cf, 0x1a2119, 3));
    const key = new THREE.DirectionalLight(0xffe4aa, 3);
    key.position.set(4, 6, 5);
    this.scene.add(key);
    this.scene.add(new THREE.GridHelper(80, 12, 0x6f5d31, 0x283128));
    this.group.add(...this.geometry(part));
    this.scene.add(this.group);
    this.camera.position.set(34, 25, 38);
    this.controls.target.set(0, 5, 0);
    this.controls.update();
    window.addEventListener("resize", this.resize);
    this.resize();
    this.animate();
  }

  dispose(): void {
    cancelAnimationFrame(this.frame);
    window.removeEventListener("resize", this.resize);
    this.controls.dispose();
    this.group.traverse((object) => {
      if (!(object instanceof THREE.Mesh)) return;
      object.geometry.dispose();
      const materials = Array.isArray(object.material) ? object.material : [object.material];
      materials.forEach((material) => material.dispose());
    });
    this.renderer.dispose();
  }

  private geometry(part: DemoPart): THREE.Mesh[] {
    const metal = new THREE.MeshStandardMaterial({ color: 0xbfc2b8, roughness: 0.48, metalness: 0.35 });
    const dark = new THREE.MeshStandardMaterial({ color: 0x2d332d, roughness: 0.6, metalness: 0.5 });
    if (part === "rail") {
      const base = new THREE.Mesh(new THREE.BoxGeometry(42, 5, 13), metal);
      base.position.y = 4;
      const ridge = new THREE.Mesh(new THREE.BoxGeometry(34, 6, 5), dark);
      ridge.position.y = 9;
      return [base, ridge];
    }
    const base = new THREE.Mesh(new THREE.BoxGeometry(28, 4, 18), metal);
    base.position.y = 2;
    const upright = new THREE.Mesh(new THREE.BoxGeometry(5, 22, 18), metal);
    upright.position.set(-11.5, 11, 0);
    const brace = new THREE.Mesh(new THREE.BoxGeometry(17, 4, 6), dark);
    brace.position.set(-3, 10, 0);
    brace.rotation.z = Math.PI / 4;
    return [base, upright, brace];
  }

  private readonly resize = (): void => {
    const width = this.canvas.clientWidth;
    const height = this.canvas.clientHeight;
    if (width === 0 || height === 0) return;
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(width, height, false);
  };

  private readonly animate = (): void => {
    this.frame = requestAnimationFrame(this.animate);
    this.controls.update();
    this.renderer.render(this.scene, this.camera);
  };
}
