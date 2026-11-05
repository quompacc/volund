#include "png_writer.hxx"

#include <zlib.h>

#include <array>
#include <cstddef>
#include <stdexcept>

namespace volund::thumbnail {
namespace {

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
  append_u32(png, static_cast<std::uint32_t>(
                      crc32(0L, checked.data(), static_cast<uInt>(checked.size()))));
}

// PNG "Sub" filter per row: thumbnails are mostly flat colour, so horizontal
// deltas compress far better than raw rows.
constexpr std::size_t bytes_per_pixel = 4U;

std::vector<std::uint8_t> filtered_rows(int width, int height,
                                        const std::vector<std::uint8_t>& rgba) {
  const auto stride = static_cast<std::size_t>(width) * bytes_per_pixel;
  std::vector<std::uint8_t> raw;
  raw.reserve((stride + 1U) * static_cast<std::size_t>(height));
  for (std::size_t row = 0; row < static_cast<std::size_t>(height); ++row) {
    raw.push_back(1);
    const auto* line = rgba.data() + row * stride;
    for (std::size_t index = 0; index < stride; ++index) {
      const std::uint8_t left = index >= bytes_per_pixel ? line[index - bytes_per_pixel] : 0;
      raw.push_back(static_cast<std::uint8_t>(line[index] - left));
    }
  }
  return raw;
}

}  // namespace

std::vector<std::uint8_t> encode_png_rgba(int width, int height,
                                          const std::vector<std::uint8_t>& rgba) {
  if (width <= 0 || height <= 0 ||
      rgba.size() != static_cast<std::size_t>(width) * static_cast<std::size_t>(height) * bytes_per_pixel) {
    throw std::invalid_argument("PNG pixel buffer does not match its dimensions");
  }
  const auto raw = filtered_rows(width, height, rgba);
  auto compressed_size = compressBound(static_cast<uLong>(raw.size()));
  std::vector<std::uint8_t> compressed(compressed_size);
  if (compress2(compressed.data(), &compressed_size, raw.data(),
                static_cast<uLong>(raw.size()), Z_BEST_COMPRESSION) != Z_OK) {
    throw std::runtime_error("cannot compress thumbnail pixels");
  }
  compressed.resize(compressed_size);

  std::vector<std::uint8_t> png{137, 80, 78, 71, 13, 10, 26, 10};
  std::vector<std::uint8_t> header;
  append_u32(header, static_cast<std::uint32_t>(width));
  append_u32(header, static_cast<std::uint32_t>(height));
  header.insert(header.end(), {8, 6, 0, 0, 0});  // 8-bit RGBA
  chunk(png, {'I', 'H', 'D', 'R'}, header);
  chunk(png, {'I', 'D', 'A', 'T'}, compressed);
  chunk(png, {'I', 'E', 'N', 'D'}, {});
  return png;
}

}  // namespace volund::thumbnail
