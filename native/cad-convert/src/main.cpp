#include "sha256.hxx"
#include "importer.hxx"
#include "options.hxx"
#include "thumbnail.hxx"

#include <BRepBndLib.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <Message_ProgressRange.hxx>
#include <Poly_Triangulation.hxx>
#include <Precision.hxx>
#include <Quantity_Color.hxx>
#include <RWGltf_CafWriter.hxx>
#include <RWMesh_CoordinateSystem.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <TColStd_IndexedDataMapOfStringString.hxx>
#include <TCollection_AsciiString.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TDF_LabelSequence.hxx>
#include <TDF_Tool.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Shape.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_ColorType.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <optional>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <vector>

namespace {
namespace fs = std::filesystem;

constexpr int contract_version = 1;

#ifndef VOLUND_CAD_CONVERT_VERSION
#define VOLUND_CAD_CONVERT_VERSION "development"
#endif

struct MeshSettings {
  std::string profile;
  double model_diagonal{};
  double linear_deflection{};
  double angular_deflection{};
};

struct Color {
  double red{};
  double green{};
  double blue{};
};

struct Node {
  std::string id;
  std::string name;
  std::string kind;
  std::string definition;
  std::array<double, 16> transform{};
  std::optional<Color> color;
  std::vector<Node> children;
};

struct Definition {
  std::string id;
  std::string name;
  std::string kind;
  std::optional<Color> color;
};

struct AssemblyData {
  std::vector<Node> roots;
  std::map<std::string, Definition> definitions;
  std::uint64_t instance_count{};
  std::uint64_t node_count{};
};

std::string json_escape(std::string_view value) {
  std::ostringstream output;
  output << std::hex << std::setfill('0');
  for (const unsigned char byte : value) {
    switch (byte) {
      case '"':
        output << "\\\"";
        break;
      case '\\':
        output << "\\\\";
        break;
      case '\b':
        output << "\\b";
        break;
      case '\f':
        output << "\\f";
        break;
      case '\n':
        output << "\\n";
        break;
      case '\r':
        output << "\\r";
        break;
      case '\t':
        output << "\\t";
        break;
      default:
        if (byte < 0x20U) {
          output << "\\u00" << std::setw(2) << static_cast<unsigned int>(byte);
        } else {
          output << static_cast<char>(byte);
        }
    }
  }
  return output.str();
}

std::string utf8(const TCollection_ExtendedString& value) {
  std::vector<char> buffer(static_cast<std::size_t>(value.LengthOfCString()) + 1U);
  Standard_PCharacter pointer = buffer.data();
  value.ToUTF8CString(pointer);
  return buffer.data();
}

std::string label_id(const TDF_Label& label) {
  TCollection_AsciiString entry;
  TDF_Tool::Entry(label, entry);
  return entry.ToCString();
}

std::optional<std::string> explicit_name(const TDF_Label& label) {
  Handle(TDataStd_Name) attribute;
  if (!label.FindAttribute(TDataStd_Name::GetID(), attribute) || attribute.IsNull()) {
    return std::nullopt;
  }
  const auto name = utf8(attribute->Get());
  if (name.empty()) {
    return std::nullopt;
  }
  return name;
}

std::string label_name(const TDF_Label& primary, const TDF_Label& fallback) {
  if (const auto name = explicit_name(primary)) {
    return *name;
  }
  if (!fallback.IsNull()) {
    if (const auto name = explicit_name(fallback)) {
      return *name;
    }
  }
  return label_id(fallback.IsNull() ? primary : fallback);
}

std::optional<Color> label_color(const TDF_Label& primary,
                                 const TDF_Label& fallback) {
  constexpr std::array<XCAFDoc_ColorType, 3> types{
      XCAFDoc_ColorGen, XCAFDoc_ColorSurf, XCAFDoc_ColorCurv};
  for (const auto type : types) {
    Quantity_Color color;
    if (XCAFDoc_ColorTool::GetColor(primary, type, color) ||
        (!fallback.IsNull() && XCAFDoc_ColorTool::GetColor(fallback, type, color))) {
      Standard_Real red = 0.0;
      Standard_Real green = 0.0;
      Standard_Real blue = 0.0;
      color.Values(red, green, blue, Quantity_TOC_sRGB);
      return Color{red, green, blue};
    }
  }
  return std::nullopt;
}

std::array<double, 16> matrix(const TopLoc_Location& location) {
  const auto transform = location.Transformation();
  return {
      transform.Value(1, 1), transform.Value(1, 2), transform.Value(1, 3),
      transform.Value(1, 4), transform.Value(2, 1), transform.Value(2, 2),
      transform.Value(2, 3), transform.Value(2, 4), transform.Value(3, 1),
      transform.Value(3, 2), transform.Value(3, 3), transform.Value(3, 4),
      0.0,                   0.0,                   0.0,
      1.0,
  };
}

void register_definition(const TDF_Label& label, AssemblyData& data) {
  const auto id = label_id(label);
  if (data.definitions.contains(id)) {
    return;
  }
  data.definitions.emplace(
      id, Definition{id, label_name(label, TDF_Label()),
                     XCAFDoc_ShapeTool::IsAssembly(label) ? "assembly" : "part",
                     label_color(label, TDF_Label())});
}

Node make_node(const TDF_Label& label, AssemblyData& data,
               std::set<std::string>& active_assemblies, std::size_t depth) {
  if (depth > 256) {
    throw std::runtime_error("assembly nesting exceeds 256 levels");
  }

  TDF_Label definition = label;
  const bool is_reference = XCAFDoc_ShapeTool::IsReference(label);
  if (is_reference && !XCAFDoc_ShapeTool::GetReferredShape(label, definition)) {
    throw std::runtime_error("XCAF reference has no referred shape: " +
                             label_id(label));
  }

  register_definition(definition, data);
  const bool is_assembly = XCAFDoc_ShapeTool::IsAssembly(definition);
  Node node{
      "node-" + std::to_string(++data.node_count),
      label_name(label, definition),
      is_reference ? (is_assembly ? "assembly-instance" : "part-instance")
                   : (is_assembly ? "assembly" : "part"),
      label_id(definition),
      matrix(XCAFDoc_ShapeTool::GetLocation(label)),
      label_color(label, definition),
      {},
  };

  if (is_reference) {
    ++data.instance_count;
  }
  if (!is_assembly) {
    return node;
  }

  const auto definition_id = label_id(definition);
  if (!active_assemblies.insert(definition_id).second) {
    throw std::runtime_error("cyclic assembly reference detected at " + definition_id);
  }
  TDF_LabelSequence components;
  XCAFDoc_ShapeTool::GetComponents(definition, components, Standard_False);
  node.children.reserve(static_cast<std::size_t>(components.Length()));
  for (Standard_Integer index = 1; index <= components.Length(); ++index) {
    node.children.push_back(
        make_node(components.Value(index), data, active_assemblies, depth + 1));
  }
  active_assemblies.erase(definition_id);
  return node;
}

AssemblyData extract_assembly(const Handle(TDocStd_Document)& document) {
  AssemblyData data;
  const auto shape_tool = XCAFDoc_DocumentTool::ShapeTool(document->Main());
  TDF_LabelSequence roots;
  shape_tool->GetFreeShapes(roots);
  if (roots.IsEmpty()) {
    throw std::runtime_error("CAD import produced no free shapes");
  }

  data.roots.reserve(static_cast<std::size_t>(roots.Length()));
  std::set<std::string> active_assemblies;
  for (Standard_Integer index = 1; index <= roots.Length(); ++index) {
    data.roots.push_back(
        make_node(roots.Value(index), data, active_assemblies, 0));
  }
  return data;
}

MeshSettings resolve_mesh_settings(const Handle(TDocStd_Document)& document,
                                   const Options& options) {
  const auto shape_tool = XCAFDoc_DocumentTool::ShapeTool(document->Main());
  TDF_LabelSequence roots;
  shape_tool->GetFreeShapes(roots);
  Bnd_Box bounds;
  for (Standard_Integer index = 1; index <= roots.Length(); ++index) {
    BRepBndLib::Add(XCAFDoc_ShapeTool::GetShape(roots.Value(index)), bounds);
  }
  if (bounds.IsVoid() || bounds.IsOpen()) {
    throw std::runtime_error("cannot derive finite model bounds for adaptive meshing");
  }

  Standard_Real minimum_x = 0.0;
  Standard_Real minimum_y = 0.0;
  Standard_Real minimum_z = 0.0;
  Standard_Real maximum_x = 0.0;
  Standard_Real maximum_y = 0.0;
  Standard_Real maximum_z = 0.0;
  bounds.Get(minimum_x, minimum_y, minimum_z, maximum_x, maximum_y, maximum_z);
  const auto extent_x = maximum_x - minimum_x;
  const auto extent_y = maximum_y - minimum_y;
  const auto extent_z = maximum_z - minimum_z;
  const auto diagonal =
      std::sqrt(extent_x * extent_x + extent_y * extent_y + extent_z * extent_z);
  if (!std::isfinite(diagonal) || diagonal <= Precision::Confusion()) {
    throw std::runtime_error("model bounds are too small for adaptive meshing");
  }

  const auto divisor = options.profile == "fine" ? 5000.0 : 1000.0;
  const auto default_angle = options.profile == "fine" ? 0.5 : 0.8;
  return MeshSettings{
      options.profile,
      diagonal,
      options.linear_deflection.value_or(
          std::max(diagonal / divisor, Precision::Confusion() * 10.0)),
      options.angular_deflection.value_or(default_angle),
  };
}

std::uint64_t mesh_document(const Handle(TDocStd_Document)& document,
                            double linear_deflection,
                            double angular_deflection) {
  const auto shape_tool = XCAFDoc_DocumentTool::ShapeTool(document->Main());
  TDF_LabelSequence roots;
  shape_tool->GetFreeShapes(roots);
  std::uint64_t triangle_count = 0;
  for (Standard_Integer root_index = 1; root_index <= roots.Length(); ++root_index) {
    const auto shape = XCAFDoc_ShapeTool::GetShape(roots.Value(root_index));
    BRepMesh_IncrementalMesh mesher(shape, linear_deflection, Standard_False,
                                    angular_deflection, Standard_True);
    if (!mesher.IsDone()) {
      throw std::runtime_error("OCCT meshing failed for root " +
                               label_id(roots.Value(root_index)));
    }
    for (TopExp_Explorer explorer(shape, TopAbs_FACE); explorer.More();
         explorer.Next()) {
      TopLoc_Location location;
      const auto triangulation =
          BRep_Tool::Triangulation(TopoDS::Face(explorer.Current()), location);
      if (!triangulation.IsNull()) {
        triangle_count += static_cast<std::uint64_t>(triangulation->NbTriangles());
      }
    }
  }
  return triangle_count;
}

void write_color(std::ostream& output, const std::optional<Color>& color) {
  if (!color) {
    output << "null";
    return;
  }
  output << '[' << color->red << ',' << color->green << ',' << color->blue << ']';
}

void write_node(std::ostream& output, const Node& node, int indentation) {
  const std::string indent(static_cast<std::size_t>(indentation), ' ');
  const std::string child_indent(static_cast<std::size_t>(indentation + 2), ' ');
  output << indent << "{\n"
         << child_indent << "\"id\": \"" << json_escape(node.id) << "\",\n"
         << child_indent << "\"name\": \"" << json_escape(node.name) << "\",\n"
         << child_indent << "\"kind\": \"" << node.kind << "\",\n"
         << child_indent << "\"definition\": \""
         << json_escape(node.definition) << "\",\n"
         << child_indent << "\"transform\": [";
  for (std::size_t index = 0; index < node.transform.size(); ++index) {
    output << (index == 0 ? "" : ",") << node.transform[index];
  }
  output << "],\n" << child_indent << "\"color\": ";
  write_color(output, node.color);
  output << ",\n" << child_indent << "\"children\": [";
  if (!node.children.empty()) {
    output << '\n';
    for (std::size_t index = 0; index < node.children.size(); ++index) {
      write_node(output, node.children[index], indentation + 4);
      output << (index + 1 == node.children.size() ? "\n" : ",\n");
    }
    output << child_indent;
  }
  output << "]\n" << indent << '}';
}

void write_assembly(const fs::path& path, const AssemblyData& data) {
  std::ofstream output(path, std::ios::binary);
  if (!output) {
    throw std::runtime_error("cannot create " + path.string());
  }
  output << std::setprecision(15)
         << "{\n  \"contractVersion\": 1,\n"
         << "  \"transformConvention\": \"row-major, parent-local\",\n"
         << "  \"colorSpace\": \"sRGB\",\n"
         << "  \"definitions\": [";
  if (!data.definitions.empty()) {
    output << '\n';
    std::size_t index = 0;
    for (const auto& [id, definition] : data.definitions) {
      output << "    {\"id\": \"" << json_escape(id) << "\", \"name\": \""
             << json_escape(definition.name) << "\", \"kind\": \""
             << definition.kind << "\", \"color\": ";
      write_color(output, definition.color);
      output << ", \"properties\": {\"labelEntry\": \"" << json_escape(id)
             << "\"}}" << (++index == data.definitions.size() ? "\n" : ",\n");
    }
    output << "  ";
  }
  output << "],\n  \"roots\": [";
  if (!data.roots.empty()) {
    output << '\n';
    for (std::size_t index = 0; index < data.roots.size(); ++index) {
      write_node(output, data.roots[index], 4);
      output << (index + 1 == data.roots.size() ? "\n" : ",\n");
    }
    output << "  ";
  }
  output << "]\n}\n";
  if (!output) {
    throw std::runtime_error("failed while writing " + path.string());
  }
}

void write_diagnostics(const fs::path& path, std::string_view status,
                       std::string_view severity, std::string_view code,
                       std::string_view message) {
  std::ofstream output(path, std::ios::binary);
  if (!output) {
    return;
  }
  output << "{\n  \"contractVersion\": 1,\n  \"status\": \"" << status
         << "\",\n  \"diagnostics\": [\n    {\"severity\": \"" << severity
         << "\", \"code\": \"" << json_escape(code) << "\", \"message\": \""
         << json_escape(message) << "\"}\n  ]\n}\n";
}

void write_result(const fs::path& path, const std::string& hash,
                  std::uintmax_t source_size, std::uint64_t triangle_count,
                  const AssemblyData& assembly, const MeshSettings& mesh_settings,
                  SourceFormat format) {
  std::ofstream output(path, std::ios::binary);
  if (!output) {
    throw std::runtime_error("cannot create " + path.string());
  }
  output << "{\n"
         << "  \"contractVersion\": 1,\n"
         << "  \"status\": \"ready\",\n"
         << "  \"source\": {\"sha256\": \"" << hash
         << "\", \"sizeBytes\": " << source_size
         << ", \"format\": \"" << format_name(format) << "\"},\n"
         << "  \"preview\": {\"glb\": \"preview.glb\", \"triangleCount\": "
         << triangle_count << ", \"profile\": \"" << mesh_settings.profile
         << "\", \"modelDiagonal\": " << mesh_settings.model_diagonal
         << ", \"linearDeflection\": " << mesh_settings.linear_deflection
         << ", \"angularDeflection\": " << mesh_settings.angular_deflection
         << "},\n"
         << "  \"assembly\": {\"manifest\": \"assembly.json\", "
            "\"definitionCount\": "
         << assembly.definitions.size() << ", \"instanceCount\": "
         << assembly.instance_count << "},\n"
         << "  \"diagnostics\": [{\"severity\": \"info\", "
            "\"code\": \"conversion.ready\", "
            "\"message\": \"CAD conversion completed\"}]\n"
         << "}\n";
}

int convert(const Options& options) {
  std::error_code error;
  if (!fs::is_regular_file(options.input, error) || error) {
    throw std::runtime_error("input is not a readable regular file: " +
                             options.input.string());
  }
  fs::create_directories(options.output);
  for (const auto* artifact : {"preview.glb", "thumbnail.png", "assembly.json",
                               "diagnostics.json", "result.json"}) {
    fs::remove(options.output / artifact, error);
    error.clear();
  }

  const auto hash = sha256_file(options.input);
  if (!hash) {
    throw std::runtime_error("cannot hash input file: " + options.input.string());
  }

  const auto format = resolve_source_format(options.input, options.format);
  const auto application = XCAFApp_Application::GetApplication();
  Handle(TDocStd_Document) document = import_document(options.input, format);

  auto assembly = extract_assembly(document);
  const auto mesh_settings = resolve_mesh_settings(document, options);
  const auto triangle_count = mesh_document(
      document, mesh_settings.linear_deflection, mesh_settings.angular_deflection);
  if (triangle_count == 0) {
    application->Close(document);
    throw std::runtime_error("meshing produced zero triangles");
  }

  const auto glb_path = options.output / "preview.glb";
  RWGltf_CafWriter writer(TCollection_AsciiString(glb_path.string().c_str()),
                          Standard_True);
  auto& coordinate_converter = writer.ChangeCoordinateSystemConverter();
  coordinate_converter.SetInputCoordinateSystem(RWMesh_CoordinateSystem_Zup);
  coordinate_converter.SetOutputCoordinateSystem(RWMesh_CoordinateSystem_glTF);
  writer.SetMergeFaces(true);
  writer.SetParallel(true);
  TColStd_IndexedDataMapOfStringString metadata;
  metadata.Add(TCollection_AsciiString("Generator"),
               TCollection_AsciiString("VOLUND cad-convert"));
  if (!writer.Perform(document, metadata, Message_ProgressRange())) {
    application->Close(document);
    throw std::runtime_error("OCCT could not write the GLB preview");
  }

  write_thumbnail(document, options.output / "thumbnail.png");

  write_assembly(options.output / "assembly.json", assembly);
  write_diagnostics(options.output / "diagnostics.json", "ready", "info",
                    "conversion.ready", "CAD conversion completed");
  write_result(options.output / "result.json", *hash, fs::file_size(options.input),
               triangle_count, assembly, mesh_settings, format);
  application->Close(document);
  return EXIT_SUCCESS;
}
}  // namespace

int main(int argc, char* argv[]) {
  if (argc == 2 && std::string_view(argv[1]) == "version") {
    std::cout << "volund-cad-convert " << VOLUND_CAD_CONVERT_VERSION
              << " contract-v" << contract_version
              << " occt-" << OCC_VERSION_COMPLETE << '\n';
    return EXIT_SUCCESS;
  }

  if (argc >= 2 && std::string_view(argv[1]) == "convert") {
    std::optional<fs::path> output;
    try {
      const auto options = parse_options(argc, argv);
      output = options.output;
      return convert(options);
    } catch (const Standard_Failure& failure) {
      const std::string message = failure.GetMessageString() == nullptr
                                      ? "unknown OCCT failure"
                                      : failure.GetMessageString();
      if (output) {
        fs::create_directories(*output);
        write_diagnostics(*output / "diagnostics.json", "failed", "error",
                          "occt.failure", message);
      }
      std::cerr << "conversion failed: " << message << '\n';
      return EXIT_FAILURE;
    } catch (const std::exception& exception) {
      if (output) {
        fs::create_directories(*output);
        write_diagnostics(*output / "diagnostics.json", "failed", "error",
                          "conversion.failed", exception.what());
      }
      std::cerr << "conversion failed: " << exception.what() << '\n';
      return EXIT_FAILURE;
    }
  }

  print_usage();
  return 2;
}
