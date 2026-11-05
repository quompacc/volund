// Unit tests for the GPU-free thumbnail renderer and PNG writer.
#include "../src/png_writer.hxx"
#include "../src/shaded_raster.hxx"

#include <zlib.h>

#include <cstdlib>
#include <iostream>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

using volund::thumbnail::Image;
using volund::thumbnail::Triangle;
using volund::thumbnail::TriangleSink;

namespace {
int failures = 0;

void check(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "FAIL: " << message << '\n';
    ++failures;
  }
}

// Unit cube from (0,0,0) to (1,1,1), two triangles per side, mixed winding.
std::vector<Triangle> cube() {
  const std::array<std::array<double, 3>, 8> v{{{0, 0, 0}, {1, 0, 0}, {1, 1, 0}, {0, 1, 0},
                                                {0, 0, 1}, {1, 0, 1}, {1, 1, 1}, {0, 1, 1}}};
  const std::array<std::array<int, 4>, 6> sides{{{0, 1, 2, 3}, {4, 5, 6, 7}, {0, 1, 5, 4},
                                                 {2, 3, 7, 6}, {1, 2, 6, 5}, {0, 3, 7, 4}}};
  std::vector<Triangle> triangles;
  for (const auto& s : sides) {
    triangles.push_back({v[static_cast<std::size_t>(s[0])], v[static_cast<std::size_t>(s[1])], v[static_cast<std::size_t>(s[2])]});
    triangles.push_back({v[static_cast<std::size_t>(s[2])], v[static_cast<std::size_t>(s[3])], v[static_cast<std::size_t>(s[0])]});
  }
  return triangles;
}

Image render(const std::vector<Triangle>& triangles) {
  return volund::thumbnail::render_shaded(
      [&](const TriangleSink& sink) { for (const auto& triangle : triangles) sink(triangle); }, 128, 96);
}

std::array<std::uint8_t, 3> pixel(const Image& image, int x, int y) {
  const auto index = (static_cast<std::size_t>(y * image.width + x)) * 3U;
  return {image.rgb[index], image.rgb[index + 1], image.rgb[index + 2]};
}

void renders_shaded_cube() {
  const auto image = render(cube());
  check(image.width == 128 && image.height == 96, "image has requested size");
  check(image.rgb.size() == 128U * 96U * 3U, "pixel buffer matches size");
  check(pixel(image, 0, 0) == volund::thumbnail::background_at(0, 96), "top-left corner is background");
  check(pixel(image, 127, 95) == volund::thumbnail::background_at(95, 96), "bottom-right corner is background");
  check(pixel(image, 64, 48) != volund::thumbnail::background_at(48, 96), "centre shows the model");
  std::set<std::array<std::uint8_t, 3>> colours;
  for (int y = 0; y < image.height; ++y) {
    for (int x = 0; x < image.width; ++x) {
      if (pixel(image, x, y) != volund::thumbnail::background_at(y, image.height)) colours.insert(pixel(image, x, y));
    }
  }
  check(colours.size() >= 3U, "three visible cube sides receive different shading");
}

void is_deterministic() {
  check(render(cube()).rgb == render(cube()).rgb, "rendering the same mesh twice is identical");
}

void rejects_empty_and_degenerate_meshes() {
  bool thrown = false;
  try { render({}); } catch (const std::runtime_error&) { thrown = true; }
  check(thrown, "empty mesh is rejected");
  thrown = false;
  try { render({Triangle{{{0, 0, 0}, {1, 1, 1}, {2, 2, 2}}}}); } catch (const std::runtime_error&) { thrown = true; }
  check(thrown, "collinear-only mesh is rejected");
}

void encodes_png() {
  const auto image = render(cube());
  const auto png = volund::thumbnail::encode_png_rgb(image.width, image.height, image.rgb);
  const std::vector<std::uint8_t> signature{137, 80, 78, 71, 13, 10, 26, 10};
  check(std::vector<std::uint8_t>(png.begin(), png.begin() + 8) == signature, "PNG signature");
  check(png[19] == 128 && png[23] == 96, "IHDR stores width and height");
  check(png.size() < image.rgb.size() / 4U, "pixels are compressed");
  // Decode IDAT and undo the Sub filter to prove the stream round-trips.
  const std::size_t idat_length = (static_cast<std::size_t>(png[33]) << 24U) | (static_cast<std::size_t>(png[34]) << 16U) |
                                  (static_cast<std::size_t>(png[35]) << 8U) | png[36];
  std::vector<std::uint8_t> raw((128U * 3U + 1U) * 96U);
  auto raw_size = static_cast<uLongf>(raw.size());
  check(uncompress(raw.data(), &raw_size, png.data() + 41, static_cast<uLong>(idat_length)) == Z_OK, "IDAT inflates");
  std::vector<std::uint8_t> decoded;
  for (std::size_t row = 0; row < 96U; ++row) {
    const auto* line = raw.data() + row * (128U * 3U + 1U);
    check(line[0] == 1, "rows use the Sub filter");
    for (std::size_t index = 0; index < 128U * 3U; ++index) {
      const std::uint8_t left = index >= 3U ? decoded[row * 384U + index - 3U] : 0;
      decoded.push_back(static_cast<std::uint8_t>(line[index + 1U] + left));
    }
  }
  check(decoded == image.rgb, "PNG round-trips the pixels");
  bool thrown = false;
  try { volund::thumbnail::encode_png_rgb(2, 2, {1, 2, 3}); } catch (const std::invalid_argument&) { thrown = true; }
  check(thrown, "mismatched buffer is rejected");
}
}  // namespace

int main() {
  renders_shaded_cube();
  is_deterministic();
  rejects_empty_and_degenerate_meshes();
  encodes_png();
  if (failures != 0) return EXIT_FAILURE;
  std::cout << "thumbnail tests passed\n";
  return EXIT_SUCCESS;
}
