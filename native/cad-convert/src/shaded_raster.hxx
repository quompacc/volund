#pragma once

// Deterministic CPU renderer for model thumbnails: orthographic three-quarter
// view, z-buffered flat shading with two lights, silhouette/crease outlines and
// 2x2 supersampling. It needs no GPU, display, or OCCT types, so it is unit
// tested on its own (tests/thumbnail_test.cpp).

#include <array>
#include <cstdint>
#include <functional>
#include <vector>

namespace volund::thumbnail {

using Vertex = std::array<double, 3>;          // model coordinates, Z up
using Triangle = std::array<Vertex, 3>;
using TriangleSink = std::function<void(const Triangle&)>;
// Calls the sink once per triangle. The renderer visits the source twice
// (bounds, then raster) so callers never have to materialise all triangles.
using TriangleSource = std::function<void(const TriangleSink&)>;

struct Image {
  int width = 0;
  int height = 0;
  std::vector<std::uint8_t> rgb;  // row-major, 3 bytes per pixel
};

// Throws std::runtime_error when the source yields no non-degenerate triangle.
Image render_shaded(const TriangleSource& source, int width, int height);

// Background colour of the top-left pixel; exposed for tests.
std::array<std::uint8_t, 3> background_at(int y, int height);

}  // namespace volund::thumbnail
