# VORON 2.4 assembly import benchmark

- Date: 2026-08-28
- Converter: `volund-cad-convert 0.2.0`, contract v1
- OCCT: 7.8.1
- Host: Debian 13 LXC, 4 vCPU, 8 GiB RAM
- Source: `VORON2.4_Assembly.step`
- Source size: 267,315,729 bytes
- SHA-256: `1e69d1070edd39a553126ebf55bc660f5ad76c5f24a7232cd82788b4ed5dcf19`

The private source model is excluded from Git. Both runs used a transient
systemd unit with `MemoryMax=6G`, `MemorySwapMax=1G`, `TasksMax=128`, and a
30-minute runtime limit.

## Assembly structure

| Metric | Result |
|---|---:|
| Root products | 1 |
| Definitions | 1,679 |
| Instances | 1,689 |
| Manifest nodes | 1,690 |
| Maximum hierarchy depth | 8 |
| Definitions with color | 1,325 |
| Assembly manifest | 1,179,410 bytes |

The root product name, full hierarchy, repeated instances, colors, source hash,
and transforms survived the STEP/XCAF conversion.

## Preview comparison

| Profile | Linear deflection | Angular deflection | Triangles | GLB size | Wall time | Peak RSS | Swap |
|---|---:|---:|---:|---:|---:|---:|---:|
| Fine | 0.1 | 0.5 | 2,465,106 | 73,286,896 B | 110.90 s | 5,180,448 KiB | 0 B |
| Medium | 0.5 | 0.8 | 1,500,430 | 48,506,792 B | 107.67 s | 5,114,388 KiB | 0 B |
| Adaptive web | 0.865437 | 0.8 | 1,462,828 | 47,622,296 B | 108.47 s | 5,105,072 KiB | 0 B |

The adaptive run used a measured model diagonal of `865.437` model units and
was submitted through the serialized Rust job runner. Its resolved linear
deflection is `diagonal / 1000`.

## Consequences

- STEP parsing and XCAF document construction dominate runtime and memory.
- Coarser meshing materially reduces browser payload and triangle count, but
  barely changes server memory use.
- The initial worker queue should run one large conversion at a time on this
  host. Parallelism inside one meshing job remains allowed.
- A fixed absolute deflection is not a durable default because CAD files may use
  different units and scales. Preview profiles should derive deflection from
  model bounds and retain an explicit high-quality override.
- A 48-73 MB GLB is usable for validation but too large as the only everyday web
  preview. VÖLUND needs adaptive preview quality and later mesh compression or
  progressive loading while keeping the original STEP untouched.
