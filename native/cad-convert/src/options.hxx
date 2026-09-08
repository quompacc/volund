#pragma once

#include <filesystem>
#include <optional>
#include <string>

struct Options {
  std::filesystem::path input;
  std::filesystem::path output;
  std::string profile = "web";
  std::string format = "auto";
  std::optional<double> linear_deflection;
  std::optional<double> angular_deflection;
};

Options parse_options(int argc, char* argv[]);
void print_usage();
