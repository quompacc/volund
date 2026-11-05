#include "shaded_raster.hxx"

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <limits>
#include <stdexcept>

namespace volund::thumbnail {
namespace {
constexpr int supersample = 2;
constexpr double margin = 0.07;           // free border on every side
constexpr double pi = 3.14159265358979323846;
// Between the web UI's light (#0e6f7c) and dark (#5bb8c4) accent, so the
// model reads on both theme stages; the background stays transparent.
constexpr std::array<double, 3> base_colour{79.0, 163.0, 174.0};
constexpr std::array<std::array<int, 2>, 4> neighbours{{{1, 0}, {-1, 0}, {0, 1}, {0, -1}}};

struct V {
  double x, y, z;
};
V add(V a, V b) { return {a.x + b.x, a.y + b.y, a.z + b.z}; }
V sub(V a, V b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
V mul(V a, double s) { return {a.x * s, a.y * s, a.z * s}; }
double dot(V a, V b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
V cross(V a, V b) {
  return {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x};
}
V normalize(V a) {
  const double length = std::sqrt(dot(a, a));
  return length > 0.0 ? mul(a, 1.0 / length) : V{0.0, 0.0, 0.0};
}
V from(const Vertex& vertex) { return {vertex[0], vertex[1], vertex[2]}; }

// Orthographic three-quarter view from front-right-top; Z is up.
struct Camera {
  V eye{}, right{}, up{}, key{};
  Camera() {
    const double azimuth = 35.0 * pi / 180.0;
    const double elevation = 28.0 * pi / 180.0;
    eye = {std::cos(elevation) * std::sin(azimuth),
           -std::cos(elevation) * std::cos(azimuth), std::sin(elevation)};
    const V forward = mul(eye, -1.0);
    right = normalize(cross(forward, V{0.0, 0.0, 1.0}));
    up = cross(right, forward);
    key = normalize(add(add(mul(eye, 0.45), mul(up, 1.0)), mul(right, -0.6)));
  }
  [[nodiscard]] V view(V point) const {
    return {dot(point, right), dot(point, up), dot(point, eye)};
  }
};

struct Bounds {
  double min_x = std::numeric_limits<double>::max(), max_x = std::numeric_limits<double>::lowest();
  double min_y = min_x, max_y = max_x, min_z = min_x, max_z = max_x;
  std::size_t triangles = 0;
  void include(V point) {
    min_x = std::min(min_x, point.x); max_x = std::max(max_x, point.x);
    min_y = std::min(min_y, point.y); max_y = std::max(max_y, point.y);
    min_z = std::min(min_z, point.z); max_z = std::max(max_z, point.z);
  }
};

// Two-sided Lambert shading: mesh exports often have inconsistent winding.
std::array<std::uint8_t, 3> shade(const Camera& camera, V a, V b, V c) {
  V normal = normalize(cross(sub(b, a), sub(c, a)));
  if (dot(normal, camera.eye) < 0.0) normal = mul(normal, -1.0);
  const double light = std::min(1.12, 0.3 + 0.62 * std::max(0.0, dot(normal, camera.key)) +
                                           0.18 * std::max(0.0, dot(normal, camera.eye)));
  std::array<std::uint8_t, 3> colour{};
  for (std::size_t channel = 0; channel < colour.size(); ++channel) {
    colour[channel] = static_cast<std::uint8_t>(
        std::clamp(std::lround(base_colour[channel] * light), 0L, 255L));
  }
  return colour;
}

class Raster {
 public:
  Raster(int width, int height)
      : width_(width), height_(height),
        depth_(static_cast<std::size_t>(width) * static_cast<std::size_t>(height),
               std::numeric_limits<double>::lowest()),
        colour_(depth_.size() * 3U, 0) {}

  void triangle(const std::array<V, 3>& p, const std::array<std::uint8_t, 3>& colour) {
    const double area = edge(p[0], p[1], p[2]);
    if (std::abs(area) < 1e-12) return;
    const int x0 = std::max(0, static_cast<int>(std::floor(std::min({p[0].x, p[1].x, p[2].x}))));
    const int x1 = std::min(width_ - 1, static_cast<int>(std::ceil(std::max({p[0].x, p[1].x, p[2].x}))));
    const int y0 = std::max(0, static_cast<int>(std::floor(std::min({p[0].y, p[1].y, p[2].y}))));
    const int y1 = std::min(height_ - 1, static_cast<int>(std::ceil(std::max({p[0].y, p[1].y, p[2].y}))));
    for (int y = y0; y <= y1; ++y) {
      for (int x = x0; x <= x1; ++x) {
        const V centre{x + 0.5, y + 0.5, 0.0};
        const double w0 = edge(p[1], p[2], centre) / area;
        const double w1 = edge(p[2], p[0], centre) / area;
        const double w2 = edge(p[0], p[1], centre) / area;
        if (w0 < 0.0 || w1 < 0.0 || w2 < 0.0) continue;
        const double depth = w0 * p[0].z + w1 * p[1].z + w2 * p[2].z;
        const auto index = offset(x, y);
        if (depth <= depth_[index]) continue;
        depth_[index] = depth;
        std::copy(colour.begin(), colour.end(), colour_.begin() + static_cast<std::ptrdiff_t>(index * 3U));
      }
    }
  }

  // Darken silhouettes and depth discontinuities so parts stay readable.
  void outline(double crease_depth) {
    std::vector<double> factor(depth_.size(), 1.0);
    for (int y = 0; y < height_; ++y) {
      for (int x = 0; x < width_; ++x) {
        const auto index = offset(x, y);
        if (!covered(index)) continue;
        for (const auto& [dx, dy] : neighbours) {
          const int nx = x + dx, ny = y + dy;
          if (nx < 0 || ny < 0 || nx >= width_ || ny >= height_) continue;
          const auto neighbour = offset(nx, ny);
          if (!covered(neighbour)) factor[index] = std::min(factor[index], 0.35);
          else if (depth_[neighbour] - depth_[index] > crease_depth) factor[index] = std::min(factor[index], 0.55);
        }
      }
    }
    for (std::size_t index = 0; index < factor.size(); ++index) {
      for (std::size_t channel = 0; channel < 3U; ++channel) {
        auto& value = colour_[index * 3U + channel];
        value = static_cast<std::uint8_t>(std::lround(value * factor[index]));
      }
    }
  }

  // Average each 2x2 block: colour over the covered samples, alpha from coverage.
  [[nodiscard]] Image downsample(int width, int height) const {
    Image image{width, height, std::vector<std::uint8_t>(static_cast<std::size_t>(width * height) * 4U)};
    constexpr int samples = supersample * supersample;
    for (int y = 0; y < height; ++y) {
      for (int x = 0; x < width; ++x) {
        int covered_samples = 0;
        std::array<int, 3> sum{};
        for (int sy = 0; sy < supersample; ++sy) {
          for (int sx = 0; sx < supersample; ++sx) {
            const auto index = offset(x * supersample + sx, y * supersample + sy);
            if (!covered(index)) continue;
            ++covered_samples;
            for (std::size_t channel = 0; channel < 3U; ++channel) sum[channel] += colour_[index * 3U + channel];
          }
        }
        const auto out = static_cast<std::size_t>(y * width + x) * 4U;
        for (std::size_t channel = 0; channel < 3U; ++channel) {
          image.rgba[out + channel] = covered_samples == 0 ? 0 : static_cast<std::uint8_t>(
              (sum[channel] + covered_samples / 2) / covered_samples);
        }
        image.rgba[out + 3U] = static_cast<std::uint8_t>((covered_samples * 255 + samples / 2) / samples);
      }
    }
    return image;
  }

 private:
  static double edge(V a, V b, V c) { return (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x); }
  [[nodiscard]] std::size_t offset(int x, int y) const {
    return static_cast<std::size_t>(y) * static_cast<std::size_t>(width_) + static_cast<std::size_t>(x);
  }
  [[nodiscard]] bool covered(std::size_t index) const {
    return depth_[index] > std::numeric_limits<double>::lowest();
  }

  int width_, height_;
  std::vector<double> depth_;
  std::vector<std::uint8_t> colour_;
};
}  // namespace

Image render_shaded(const TriangleSource& source, int width, int height) {
  if (width <= 0 || height <= 0) throw std::invalid_argument("thumbnail size must be positive");
  const Camera camera;
  Bounds bounds;
  source([&](const Triangle& triangle) {
    const V a = from(triangle[0]), b = from(triangle[1]), c = from(triangle[2]);
    if (dot(cross(sub(b, a), sub(c, a)), cross(sub(b, a), sub(c, a))) <= 0.0) return;
    ++bounds.triangles;
    for (const V point : {a, b, c}) bounds.include(camera.view(point));
  });
  if (bounds.triangles == 0) throw std::runtime_error("thumbnail projection contains no triangles");

  const int raster_width = width * supersample, raster_height = height * supersample;
  const double span_x = std::max(bounds.max_x - bounds.min_x, 1e-9);
  const double span_y = std::max(bounds.max_y - bounds.min_y, 1e-9);
  const double scale = std::min(raster_width * (1.0 - 2.0 * margin) / span_x,
                                raster_height * (1.0 - 2.0 * margin) / span_y);
  const double centre_x = (bounds.min_x + bounds.max_x) / 2.0;
  const double centre_y = (bounds.min_y + bounds.max_y) / 2.0;
  const auto to_screen = [&](V point) {
    const V viewed = camera.view(point);
    return V{raster_width / 2.0 + (viewed.x - centre_x) * scale,
             raster_height / 2.0 - (viewed.y - centre_y) * scale, viewed.z};
  };

  Raster raster(raster_width, raster_height);
  source([&](const Triangle& triangle) {
    const V a = from(triangle[0]), b = from(triangle[1]), c = from(triangle[2]);
    if (dot(cross(sub(b, a), sub(c, a)), cross(sub(b, a), sub(c, a))) <= 0.0) return;
    raster.triangle({to_screen(a), to_screen(b), to_screen(c)}, shade(camera, a, b, c));
  });
  // A depth jump of more than eight pixel widths marks a crease between parts.
  raster.outline(8.0 / scale);
  return raster.downsample(width, height);
}

}  // namespace volund::thumbnail
