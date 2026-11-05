#include "thumbnail.hxx"

#include "png_writer.hxx"
#include "shaded_raster.hxx"

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

#include <fstream>
#include <stdexcept>

namespace {
constexpr int width = 512;
constexpr int height = 384;

// Feed every meshed face triangle of the document's free shapes to the sink.
void visit_triangles(const Handle(TDocStd_Document)& document,
                     const volund::thumbnail::TriangleSink& sink) {
  const auto tool = XCAFDoc_DocumentTool::ShapeTool(document->Main());
  TDF_LabelSequence roots;
  tool->GetFreeShapes(roots);
  for (Standard_Integer root = 1; root <= roots.Length(); ++root) {
    const auto shape = tool->GetShape(roots.Value(root));
    for (TopExp_Explorer explorer(shape, TopAbs_FACE); explorer.More(); explorer.Next()) {
      const auto face = TopoDS::Face(explorer.Current());
      TopLoc_Location location;
      const auto triangulation = BRep_Tool::Triangulation(face, location);
      if (triangulation.IsNull()) continue;
      const auto transform = location.Transformation();
      for (Standard_Integer index = 1; index <= triangulation->NbTriangles(); ++index) {
        Standard_Integer a, b, c;
        triangulation->Triangle(index).Get(a, b, c);
        volund::thumbnail::Triangle triangle;
        const std::array<Standard_Integer, 3> nodes{a, b, c};
        for (std::size_t node = 0; node < nodes.size(); ++node) {
          const auto point = triangulation->Node(nodes[node]).Transformed(transform);
          triangle[node] = {point.X(), point.Y(), point.Z()};
        }
        sink(triangle);
      }
    }
  }
}
}  // namespace

void write_thumbnail(const Handle(TDocStd_Document)& document,
                     const std::filesystem::path& path) {
  const auto image = volund::thumbnail::render_shaded(
      [&](const volund::thumbnail::TriangleSink& sink) { visit_triangles(document, sink); },
      width, height);
  const auto png = volund::thumbnail::encode_png_rgba(image.width, image.height, image.rgba);
  std::ofstream output(path, std::ios::binary);
  output.write(reinterpret_cast<const char*>(png.data()), static_cast<std::streamsize>(png.size()));
  if (!output) throw std::runtime_error("cannot write deterministic thumbnail");
}
