#pragma once

#include <cstdint>
#include <vector>

namespace volund::thumbnail {

// Encode 8-bit RGB pixels as a deterministic, zlib-compressed PNG.
// Throws std::invalid_argument for inconsistent sizes and std::runtime_error
// when compression fails.
std::vector<std::uint8_t> encode_png_rgb(int width, int height,
                                         const std::vector<std::uint8_t>& rgb);

}  // namespace volund::thumbnail
