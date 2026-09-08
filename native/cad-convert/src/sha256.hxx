#pragma once

#include <filesystem>
#include <optional>
#include <string>

std::optional<std::string> sha256_file(const std::filesystem::path& path);
