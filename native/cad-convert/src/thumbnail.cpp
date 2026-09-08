#include "thumbnail.hxx"

#include <BRep_Tool.hxx>
#include <Poly_Triangulation.hxx>
#include <TDF_LabelSequence.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Face.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <limits>
#include <stdexcept>
#include <vector>

namespace {
constexpr int width = 512;
constexpr int height = 384;
constexpr std::size_t max_segments = 600000;
using Point = std::array<double, 2>;
using Segment = std::array<Point, 2>;

std::uint32_t crc32(const std::vector<std::uint8_t>& bytes) {
  std::uint32_t crc = 0xffffffffU;
  for (const auto byte : bytes) {
    crc ^= byte;
    for (int bit = 0; bit < 8; ++bit) {
      crc = (crc >> 1U) ^ (0xedb88320U & (0U - (crc & 1U)));
    }
  }
  return ~crc;
}

void append_u32(std::vector<std::uint8_t>& bytes, std::uint32_t value) {
  bytes.push_back(static_cast<std::uint8_t>(value >> 24U));
  bytes.push_back(static_cast<std::uint8_t>(value >> 16U));
  bytes.push_back(static_cast<std::uint8_t>(value >> 8U));
  bytes.push_back(static_cast<std::uint8_t>(value));
}

void chunk(std::vector<std::uint8_t>& png, const std::array<char, 4>& kind,
           const std::vector<std::uint8_t>& data) {
  append_u32(png, static_cast<std::uint32_t>(data.size()));
  std::vector<std::uint8_t> checked(kind.begin(), kind.end());
  checked.insert(checked.end(), data.begin(), data.end());
  png.insert(png.end(), checked.begin(), checked.end());
  append_u32(png, crc32(checked));
}

std::vector<std::uint8_t> zlib_store(const std::vector<std::uint8_t>& raw) {
  std::vector<std::uint8_t> result{0x78, 0x01};
  std::size_t offset = 0;
  while (offset < raw.size()) {
    const auto count = std::min<std::size_t>(65535, raw.size() - offset);
    result.push_back(offset + count == raw.size() ? 1 : 0);
    result.push_back(static_cast<std::uint8_t>(count));
    result.push_back(static_cast<std::uint8_t>(count >> 8U));
    const auto inverted = static_cast<std::uint16_t>(~static_cast<std::uint16_t>(count));
    result.push_back(static_cast<std::uint8_t>(inverted));
    result.push_back(static_cast<std::uint8_t>(inverted >> 8U));
    result.insert(result.end(), raw.begin() + static_cast<std::ptrdiff_t>(offset),
                  raw.begin() + static_cast<std::ptrdiff_t>(offset + count));
    offset += count;
  }
  std::uint32_t a = 1;
  std::uint32_t b = 0;
  for (const auto byte : raw) {
    a = (a + byte) % 65521U;
    b = (b + a) % 65521U;
  }
  append_u32(result, (b << 16U) | a);
  return result;
}

void line(std::vector<std::uint8_t>& pixels, Point from, Point to) {
  int x0 = static_cast<int>(std::lround(from[0]));
  int y0 = static_cast<int>(std::lround(from[1]));
  const int x1 = static_cast<int>(std::lround(to[0]));
  const int y1 = static_cast<int>(std::lround(to[1]));
  const int dx = std::abs(x1 - x0), sx = x0 < x1 ? 1 : -1;
  const int dy = -std::abs(y1 - y0), sy = y0 < y1 ? 1 : -1;
  int error = dx + dy;
  for (;;) {
    if (x0 >= 0 && x0 < width && y0 >= 0 && y0 < height) {
      const auto index = static_cast<std::size_t>((y0 * width + x0) * 3);
      pixels[index] = 229;
      pixels[index + 1] = 170;
      pixels[index + 2] = 70;
    }
    if (x0 == x1 && y0 == y1) break;
    const int doubled = 2 * error;
    if (doubled >= dy) { error += dy; x0 += sx; }
    if (doubled <= dx) { error += dx; y0 += sy; }
  }
}

std::vector<Segment> project(const Handle(TDocStd_Document)& document) {
  std::vector<Segment> segments;
  TDF_LabelSequence roots;
  XCAFDoc_DocumentTool::ShapeTool(document->Main())->GetFreeShapes(roots);
  for (Standard_Integer root = 1; root <= roots.Length(); ++root) {
    const auto shape = XCAFDoc_DocumentTool::ShapeTool(document->Main())->GetShape(roots.Value(root));
    for (TopExp_Explorer explorer(shape, TopAbs_FACE); explorer.More(); explorer.Next()) {
      const auto face = TopoDS::Face(explorer.Current());
      TopLoc_Location location;
      const auto triangulation = BRep_Tool::Triangulation(face, location);
      if (triangulation.IsNull()) continue;
      for (Standard_Integer index = 1; index <= triangulation->NbTriangles(); ++index) {
        if (segments.size() + 3 > max_segments) return segments;
        Standard_Integer a, b, c;
        triangulation->Triangle(index).Get(a, b, c);
        std::array<Point, 3> points;
        const std::array<Standard_Integer, 3> nodes{a, b, c};
        for (std::size_t node = 0; node < nodes.size(); ++node) {
          const auto point = triangulation->Node(nodes[node]).Transformed(location.Transformation());
          points[node] = Point{point.X() - 0.62 * point.Z(),
                               point.Y() - 0.32 * (point.X() + point.Z())};
        }
        segments.push_back({points[0], points[1]});
        segments.push_back({points[1], points[2]});
        segments.push_back({points[2], points[0]});
      }
    }
  }
  return segments;
}
}  // namespace

void write_thumbnail(const Handle(TDocStd_Document)& document,
                     const std::filesystem::path& path) {
  auto segments = project(document);
  if (segments.empty()) throw std::runtime_error("thumbnail projection contains no triangles");
  double min_x = std::numeric_limits<double>::max(), min_y = min_x;
  double max_x = std::numeric_limits<double>::lowest(), max_y = max_x;
  for (const auto& segment : segments) for (const auto& point : segment) {
    min_x = std::min(min_x, point[0]); max_x = std::max(max_x, point[0]);
    min_y = std::min(min_y, point[1]); max_y = std::max(max_y, point[1]);
  }
  const double scale = std::min((width - 48.0) / std::max(max_x - min_x, 1e-9),
                                (height - 48.0) / std::max(max_y - min_y, 1e-9));
  std::vector<std::uint8_t> pixels(static_cast<std::size_t>(width * height * 3));
  for (std::size_t i = 0; i < pixels.size(); i += 3) {
    pixels[i] = 18; pixels[i + 1] = 24; pixels[i + 2] = 33;
  }
  for (auto segment : segments) {
    for (auto& point : segment) {
      point[0] = 24.0 + (point[0] - min_x) * scale;
      point[1] = height - 24.0 - (point[1] - min_y) * scale;
    }
    line(pixels, segment[0], segment[1]);
  }
  std::vector<std::uint8_t> raw;
  raw.reserve(static_cast<std::size_t>((width * 3 + 1) * height));
  for (int row = 0; row < height; ++row) {
    raw.push_back(0);
    const auto begin = pixels.begin() + static_cast<std::ptrdiff_t>(row * width * 3);
    raw.insert(raw.end(), begin, begin + width * 3);
  }
  std::vector<std::uint8_t> png{137, 80, 78, 71, 13, 10, 26, 10};
  std::vector<std::uint8_t> header;
  append_u32(header, width); append_u32(header, height);
  header.insert(header.end(), {8, 2, 0, 0, 0});
  chunk(png, {'I', 'H', 'D', 'R'}, header);
  chunk(png, {'I', 'D', 'A', 'T'}, zlib_store(raw));
  chunk(png, {'I', 'E', 'N', 'D'}, {});
  std::ofstream output(path, std::ios::binary);
  output.write(reinterpret_cast<const char*>(png.data()), static_cast<std::streamsize>(png.size()));
  if (!output) throw std::runtime_error("cannot write deterministic thumbnail");
}
