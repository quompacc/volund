#include "importer.hxx"

#include <BRepTools.hxx>
#include <BRep_Builder.hxx>
#include <IGESCAFControl_Reader.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Message_ProgressRange.hxx>
#include <Poly_Triangle.hxx>
#include <Poly_Triangulation.hxx>
#include <Quantity_Color.hxx>
#include <RWGltf_CafReader.hxx>
#include <RWObj_CafReader.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <TCollection_AsciiString.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TDataStd_Name.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Face.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include "three_mf.hxx"

#include <assimp/Importer.hpp>
#include <assimp/material.h>
#include <assimp/postprocess.h>
#include <assimp/scene.h>
#include <gp_Dir.hxx>
#include <gp_Trsf.hxx>

#include <algorithm>
#include <cctype>
#include <fstream>
#include <map>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
namespace fs = std::filesystem;

Handle(TDocStd_Document) new_document() {
  Handle(TDocStd_Document) document;
  XCAFApp_Application::GetApplication()->NewDocument(
      TCollection_ExtendedString("BinXCAF"), document);
  return document;
}

std::string lower(std::string value) {
  std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
    return static_cast<char>(std::tolower(ch));
  });
  return value;
}

void set_name(const TDF_Label& label, const std::string& name) {
  if (!name.empty()) {
    TDataStd_Name::Set(label, TCollection_ExtendedString(name.c_str(), true));
  }
}

void add_single_shape(const Handle(TDocStd_Document)& document,
                      const TopoDS_Shape& shape, const std::string& name) {
  if (shape.IsNull()) {
    throw std::runtime_error("importer produced an empty shape");
  }
  const auto shape_tool = XCAFDoc_DocumentTool::ShapeTool(document->Main());
  const auto label = shape_tool->AddShape(shape, Standard_True);
  set_name(label, name);
}

void import_step(const fs::path& path, const Handle(TDocStd_Document)& document) {
  STEPCAFControl_Reader reader;
  reader.SetColorMode(Standard_True);
  reader.SetNameMode(Standard_True);
  reader.SetLayerMode(Standard_True);
  reader.SetPropsMode(Standard_True);
  if (reader.ReadFile(path.string().c_str()) != IFSelect_RetDone ||
      !reader.Transfer(document)) {
    throw std::runtime_error("OCCT could not import STEP data");
  }
}

void import_iges(const fs::path& path, const Handle(TDocStd_Document)& document) {
  IGESCAFControl_Reader reader;
  reader.SetColorMode(Standard_True);
  reader.SetNameMode(Standard_True);
  reader.SetLayerMode(Standard_True);
  if (!reader.Perform(path.string().c_str(), document, Message_ProgressRange())) {
    throw std::runtime_error("OCCT could not import IGES data");
  }
}

void import_brep(const fs::path& path, const Handle(TDocStd_Document)& document) {
  TopoDS_Shape shape;
  BRep_Builder builder;
  if (!BRepTools::Read(shape, path.string().c_str(), builder)) {
    throw std::runtime_error("OCCT could not import BREP data");
  }
  add_single_shape(document, shape, path.stem().string());
}

template <typename Reader>
void import_rwmesh(const fs::path& path, const Handle(TDocStd_Document)& document,
                   std::string_view label) {
  Reader reader;
  reader.SetDocument(document);
  reader.SetRootPrefix(TCollection_AsciiString(path.stem().string().c_str()));
  if (!reader.Perform(TCollection_AsciiString(path.string().c_str()),
                      Message_ProgressRange())) {
    throw std::runtime_error("OCCT could not import " + std::string(label) + " data");
  }
}

TopoDS_Face mesh_face(const aiMesh& mesh) {
  if (mesh.mNumVertices == 0 || mesh.mNumFaces == 0) {
    throw std::runtime_error("mesh contains no triangles");
  }
  const auto triangulation = new Poly_Triangulation(
      static_cast<Standard_Integer>(mesh.mNumVertices),
      static_cast<Standard_Integer>(mesh.mNumFaces), Standard_False, mesh.HasNormals());
  for (unsigned int index = 0; index < mesh.mNumVertices; ++index) {
    const auto& vertex = mesh.mVertices[index];
    triangulation->SetNode(static_cast<Standard_Integer>(index + 1),
                           gp_Pnt(vertex.x, vertex.y, vertex.z));
    if (mesh.HasNormals()) {
      const auto& normal = mesh.mNormals[index];
      const double length = normal.SquareLength();
      if (length > 1.0e-20) {
        triangulation->SetNormal(static_cast<Standard_Integer>(index + 1),
                                 gp_Dir(normal.x, normal.y, normal.z));
      }
    }
  }
  for (unsigned int index = 0; index < mesh.mNumFaces; ++index) {
    const auto& face = mesh.mFaces[index];
    if (face.mNumIndices != 3) {
      throw std::runtime_error("Assimp did not triangulate a mesh face");
    }
    triangulation->SetTriangle(
        static_cast<Standard_Integer>(index + 1),
        Poly_Triangle(static_cast<Standard_Integer>(face.mIndices[0] + 1),
                      static_cast<Standard_Integer>(face.mIndices[1] + 1),
                      static_cast<Standard_Integer>(face.mIndices[2] + 1)));
  }
  BRep_Builder builder;
  TopoDS_Face face;
  builder.MakeFace(face, triangulation);
  return face;
}

TopLoc_Location location_from_assimp(const aiMatrix4x4& matrix) {
  gp_Trsf transform;
  transform.SetValues(matrix.a1, matrix.a2, matrix.a3, matrix.a4,
                      matrix.b1, matrix.b2, matrix.b3, matrix.b4,
                      matrix.c1, matrix.c2, matrix.c3, matrix.c4);
  return TopLoc_Location(transform);
}

std::optional<Quantity_Color> material_color(const aiScene& scene,
                                             unsigned int material_index) {
  if (material_index >= scene.mNumMaterials) {
    return std::nullopt;
  }
  aiColor3D color;
  if (scene.mMaterials[material_index]->Get(AI_MATKEY_BASE_COLOR, color) != AI_SUCCESS &&
      scene.mMaterials[material_index]->Get(AI_MATKEY_COLOR_DIFFUSE, color) != AI_SUCCESS) {
    return std::nullopt;
  }
  return Quantity_Color(color.r, color.g, color.b, Quantity_TOC_sRGB);
}

TDF_Label add_assimp_node(const aiScene& scene, const aiNode& node,
                          const Handle(XCAFDoc_ShapeTool)& shape_tool,
                          const std::vector<TDF_Label>& mesh_labels) {
  const TDF_Label definition = shape_tool->NewShape();
  set_name(definition, node.mName.length > 0 ? node.mName.C_Str() : "node");
  for (unsigned int index = 0; index < node.mNumMeshes; ++index) {
    const unsigned int mesh_index = node.mMeshes[index];
    if (mesh_index >= mesh_labels.size()) {
      throw std::runtime_error("scene node references an invalid mesh");
    }
    if (mesh_labels[mesh_index].IsNull()) {
      continue;
    }
    const auto occurrence = shape_tool->AddComponent(
        definition, mesh_labels[mesh_index], TopLoc_Location());
    set_name(occurrence, scene.mMeshes[mesh_index]->mName.C_Str());
  }
  for (unsigned int index = 0; index < node.mNumChildren; ++index) {
    const aiNode& child = *node.mChildren[index];
    const auto child_definition = add_assimp_node(scene, child, shape_tool, mesh_labels);
    const auto occurrence = shape_tool->AddComponent(
        definition, child_definition, location_from_assimp(child.mTransformation));
    set_name(occurrence, child.mName.C_Str());
  }
  return definition;
}

void import_assimp(const fs::path& path, const Handle(TDocStd_Document)& document,
                   std::string_view format) {
  Assimp::Importer importer;
  const aiScene* scene = importer.ReadFile(
      path.string(), aiProcess_Triangulate | aiProcess_JoinIdenticalVertices |
                         aiProcess_SortByPType | aiProcess_GenSmoothNormals |
                         aiProcess_ValidateDataStructure);
  if (scene == nullptr || scene->mRootNode == nullptr) {
    throw std::runtime_error("Assimp could not import " + std::string(format) +
                             ": " + importer.GetErrorString());
  }

  const auto shape_tool = XCAFDoc_DocumentTool::ShapeTool(document->Main());
  const auto color_tool = XCAFDoc_DocumentTool::ColorTool(document->Main());
  std::vector<TDF_Label> mesh_labels(scene->mNumMeshes);
  std::size_t triangle_mesh_count = 0;
  for (unsigned int index = 0; index < scene->mNumMeshes; ++index) {
    const aiMesh& mesh = *scene->mMeshes[index];
    if ((mesh.mPrimitiveTypes & aiPrimitiveType_TRIANGLE) == 0 ||
        mesh.mNumVertices == 0 || mesh.mNumFaces == 0) {
      continue;
    }
    const auto label = shape_tool->AddShape(mesh_face(mesh), Standard_False);
    set_name(label, mesh.mName.length > 0 ? mesh.mName.C_Str()
                                         : "mesh-" + std::to_string(index + 1));
    if (const auto color = material_color(*scene, mesh.mMaterialIndex)) {
      color_tool->SetColor(label, *color, XCAFDoc_ColorSurf);
    }
    mesh_labels[index] = label;
    ++triangle_mesh_count;
  }
  if (triangle_mesh_count == 0) {
    throw std::runtime_error("Assimp imported no triangle meshes");
  }

  const auto root_definition =
      add_assimp_node(*scene, *scene->mRootNode, shape_tool, mesh_labels);
  if (scene->mRootNode->mTransformation != aiMatrix4x4()) {
    const auto wrapper = shape_tool->NewShape();
    set_name(wrapper, path.stem().string());
    shape_tool->AddComponent(wrapper, root_definition,
                             location_from_assimp(scene->mRootNode->mTransformation));
  }
  shape_tool->UpdateAssemblies();
}
TopoDS_Face mesh_face(const std::vector<volund::three_mf::Vertex>& vertices,
                      const std::vector<std::array<int, 3>>& triangles) {
  const auto triangulation = new Poly_Triangulation(static_cast<Standard_Integer>(vertices.size()),
                                                    static_cast<Standard_Integer>(triangles.size()), Standard_False);
  for (std::size_t index = 0; index < vertices.size(); ++index) {
    const auto& v = vertices[index];
    triangulation->SetNode(static_cast<Standard_Integer>(index + 1), gp_Pnt(v[0], v[1], v[2]));
  }
  for (std::size_t index = 0; index < triangles.size(); ++index) {
    const auto& t = triangles[index];
    triangulation->SetTriangle(static_cast<Standard_Integer>(index + 1), Poly_Triangle(t[0] + 1, t[1] + 1, t[2] + 1));
  }
  BRep_Builder builder;
  TopoDS_Face face;
  builder.MakeFace(face, triangulation);
  return face;
}

TopLoc_Location location_from_3mf(const volund::three_mf::Transform& t) {
  gp_Trsf transform;
  transform.SetValues(t[0], t[3], t[6], t[9], t[1], t[4], t[7], t[10], t[2], t[5], t[8], t[11]);
  return TopLoc_Location(transform);
}

class ThreeMfAssembler {
 public:
  ThreeMfAssembler(const volund::three_mf::Scene& scene, const Handle(XCAFDoc_ShapeTool)& tool, std::string fallback)
      : scene_(scene), tool_(tool), fallback_(std::move(fallback)), labels_(scene.objects.size()) {}

  // One shared XCAF definition per 3MF object; rigid placements become
  // located components, scaled or mirrored ones are baked into the mesh.
  TDF_Label definition(std::size_t index) {
    if (!labels_[index].IsNull()) return labels_[index];
    const auto& object = scene_.objects[index];
    const auto name = object.name.empty() ? fallback_ + "-" + std::to_string(index + 1) : object.name;
    TDF_Label label;
    if (object.components.empty() && object.triangles.empty()) {
      return labels_[index];  // empty object (slicers write placeholders): nothing to show
    }
    if (object.components.empty()) {
      label = tool_->AddShape(mesh_face(object.vertices, object.triangles), Standard_False);
    } else {
      // Parts before the assembly that uses them, matching the Assimp path's label order.
      for (const auto& component : object.components) {
        if (volund::three_mf::is_rigid(component.transform)) definition(component.object);
      }
      label = tool_->NewShape();
      if (!object.triangles.empty()) {
        const auto own = tool_->AddShape(mesh_face(object.vertices, object.triangles), Standard_False);
        set_name(own, name);
        tool_->AddComponent(label, own, TopLoc_Location());
      }
      for (const auto& component : object.components) place(label, component);
    }
    set_name(label, name);
    return labels_[index] = label;
  }

  void place(const TDF_Label& parent, const volund::three_mf::Instance& instance) {
    if (volund::three_mf::is_rigid(instance.transform)) {
      const auto child = definition(instance.object);
      if (child.IsNull()) return;
      const auto occurrence = tool_->AddComponent(parent, child, location_from_3mf(instance.transform));
      Handle(TDataStd_Name) child_name;
      if (child.FindAttribute(TDataStd_Name::GetID(), child_name)) TDataStd_Name::Set(occurrence, child_name->Get());
      return;
    }
    for (const auto& mesh : volund::three_mf::flatten(scene_, instance.object, instance.transform)) {
      if (mesh.triangles.empty()) continue;
      const auto baked = tool_->AddShape(mesh_face(mesh.vertices, mesh.triangles), Standard_False);
      set_name(baked, mesh.name.empty() ? fallback_ : mesh.name);
      tool_->AddComponent(parent, baked, TopLoc_Location());
    }
  }

 private:
  const volund::three_mf::Scene& scene_;
  Handle(XCAFDoc_ShapeTool) tool_;
  std::string fallback_;
  std::vector<TDF_Label> labels_;
};

// Own reader first: Assimp does not follow the production extension's
// p:path components that slicer projects use; Assimp stays the fallback.
void import_three_mf(const fs::path& path, const Handle(TDocStd_Document)& document) {
  volund::three_mf::Scene scene;
  try {
    scene = volund::three_mf::read(path);
  } catch (const std::exception& own_error) {
    try {
      import_assimp(path, document, "3MF");
      return;
    } catch (const std::exception& assimp_error) {
      throw std::runtime_error(std::string("3MF import failed: ") + own_error.what() + "; " + assimp_error.what());
    }
  }
  const auto tool = XCAFDoc_DocumentTool::ShapeTool(document->Main());
  ThreeMfAssembler assembler(scene, tool, path.stem().string());
  const auto& build = scene.build;
  if (build.size() == 1 && build[0].transform == volund::three_mf::identity) {
    assembler.definition(build[0].object);
  } else {
    const auto root = tool->NewShape();
    set_name(root, path.stem().string());
    for (const auto& item : build) assembler.place(root, item);
  }
  tool->UpdateAssemblies();
}
}  // namespace

std::string_view format_name(SourceFormat format) {
  switch (format) {
    case SourceFormat::step: return "step";
    case SourceFormat::iges: return "iges";
    case SourceFormat::brep: return "brep";
    case SourceFormat::stl: return "stl";
    case SourceFormat::three_mf: return "3mf";
    case SourceFormat::obj: return "obj";
    case SourceFormat::ply: return "ply";
    case SourceFormat::gltf: return "gltf";
    case SourceFormat::glb: return "glb";
  }
  throw std::logic_error("unknown source format");
}

SourceFormat resolve_source_format(const fs::path& path,
                                   std::string_view requested_format) {
  std::string value = lower(std::string(requested_format));
  if (value.empty() || value == "auto") {
    value = lower(path.extension().string());
    if (!value.empty() && value.front() == '.') value.erase(value.begin());
  }
  static const std::map<std::string, SourceFormat> formats = {
      {"step", SourceFormat::step}, {"stp", SourceFormat::step},
      {"iges", SourceFormat::iges}, {"igs", SourceFormat::iges},
      {"brep", SourceFormat::brep}, {"stl", SourceFormat::stl},
      {"3mf", SourceFormat::three_mf}, {"obj", SourceFormat::obj},
      {"ply", SourceFormat::ply}, {"gltf", SourceFormat::gltf},
      {"glb", SourceFormat::glb}};
  const auto found = formats.find(value);
  if (found == formats.end()) {
    throw std::invalid_argument("unsupported CAD format: " + value);
  }
  return found->second;
}

Handle(TDocStd_Document) import_document(const fs::path& path,
                                         SourceFormat format) {
  auto document = new_document();
  switch (format) {
    case SourceFormat::step: import_step(path, document); break;
    case SourceFormat::iges: import_iges(path, document); break;
    case SourceFormat::brep: import_brep(path, document); break;
    case SourceFormat::stl: import_assimp(path, document, "STL"); break;
    case SourceFormat::obj: import_rwmesh<RWObj_CafReader>(path, document, "OBJ"); break;
    case SourceFormat::gltf:
    case SourceFormat::glb:
      import_rwmesh<RWGltf_CafReader>(path, document, "glTF");
      break;
    case SourceFormat::ply: import_assimp(path, document, "PLY"); break;
    case SourceFormat::three_mf: import_three_mf(path, document); break;
  }
  return document;
}
