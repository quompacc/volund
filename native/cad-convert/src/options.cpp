#include "options.hxx"

#include <iostream>
#include <stdexcept>
#include <string>
#include <string_view>

Options parse_options(int argc, char* argv[]) {
  Options options;
  for (int index = 2; index < argc; ++index) {
    const std::string_view argument(argv[index]);
    if ((argument == "--input" || argument == "--output" ||
         argument == "--profile" || argument == "--format" ||
         argument == "--linear-deflection" ||
         argument == "--angular-deflection") &&
        index + 1 >= argc) {
      throw std::invalid_argument(std::string(argument) + " requires a value");
    }
    if (argument == "--input") {
      options.input = argv[++index];
    } else if (argument == "--output") {
      options.output = argv[++index];
    } else if (argument == "--profile") {
      options.profile = argv[++index];
    } else if (argument == "--format") {
      options.format = argv[++index];
    } else if (argument == "--linear-deflection") {
      options.linear_deflection = std::stod(argv[++index]);
    } else if (argument == "--angular-deflection") {
      options.angular_deflection = std::stod(argv[++index]);
    } else {
      throw std::invalid_argument("unknown option: " + std::string(argument));
    }
  }

  if (options.input.empty() || options.output.empty()) {
    throw std::invalid_argument("--input and --output are required");
  }
  if (options.profile != "web" && options.profile != "fine") {
    throw std::invalid_argument("--profile must be web or fine");
  }
  if ((options.linear_deflection && !(*options.linear_deflection > 0.0)) ||
      (options.angular_deflection && !(*options.angular_deflection > 0.0))) {
    throw std::invalid_argument("mesh deflections must be positive");
  }
  return options;
}

void print_usage() {
  std::cerr << "usage:\n"
            << "  volund-cad-convert version\n"
            << "  volund-cad-convert convert --input model.step --output directory "
               "[--format auto|step|iges|brep|stl|3mf|obj|ply|gltf|glb] "
               "[--profile web|fine] [--linear-deflection value] "
               "[--angular-deflection value]\n";
}
