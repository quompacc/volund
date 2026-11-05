#pragma once

// Minimal 3MF reader (core spec plus the production extension's p:path
// components used by Bambu Studio, PrusaSlicer and OrcaSlicer). It reads the
// ZIP package with zlib and returns the object graph: shared objects with
// meshes and/or placed components, plus the build items. Keeping the graph
// (instead of flattening) preserves repeated parts for the assembly manifest.
// No OCCT types, so it is unit tested on its own (tests/three_mf_test.cpp).

#include <array>
#include <cstddef>
#include <filesystem>
#include <string>
#include <vector>

namespace volund::three_mf {

using Vertex = std::array<double, 3>;
using Transform = std::array<double, 12>;  // m00 m01 m02 m10 ... m32, row-vector convention
inline constexpr Transform identity{1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0};

struct Instance {
  std::size_t object;   // index into Scene::objects
  Transform transform;  // placement relative to the parent
};

struct Object {
  std::string name;
  std::vector<Vertex> vertices;
  std::vector<std::array<int, 3>> triangles;  // zero-based, validated vertex indices
  std::vector<Instance> components;
};

struct Scene {
  std::vector<Object> objects;  // every object reachable from the build
  std::vector<Instance> build;
};

struct Mesh {
  std::string name;
  std::vector<Vertex> vertices;
  std::vector<std::array<int, 3>> triangles;
};

// Throws std::runtime_error when the package is unreadable, references
// missing parts, or yields no triangles.
Scene read(const std::filesystem::path& path);

Vertex apply(const Transform& transform, const Vertex& point);
Transform compose(const Transform& first, const Transform& second);  // first, then second
// True for rotations plus translation (no scale, shear or mirror).
bool is_rigid(const Transform& transform);
// World-space meshes of `object` and its components placed by `transform`.
std::vector<Mesh> flatten(const Scene& scene, std::size_t object, const Transform& transform);

}  // namespace volund::three_mf
