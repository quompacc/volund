#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepTools.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <IGESCAFControl_Writer.hxx>
#include <Quantity_Color.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <STEPControl_StepModelType.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopLoc_Location.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_ColorType.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <gp_Pnt.hxx>

#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <string_view>

namespace {
TCollection_ExtendedString name(const char* utf8) {
  return TCollection_ExtendedString(utf8, Standard_True);
}
}  // namespace

int main(int argc, char* argv[]) {
  const bool empty = argc == 3 && std::string_view(argv[2]) == "--empty";
  if (argc != 2 && !empty) {
    std::cerr << "usage: volund-cad-fixture output.step|output.iges|output.brep [--empty]\n";
    return 2;
  }

  const std::filesystem::path output(argv[1]);
  if (!output.parent_path().empty()) {
    std::filesystem::create_directories(output.parent_path());
  }

  const auto application = XCAFApp_Application::GetApplication();
  Handle(TDocStd_Document) document;
  application->NewDocument(name("BinXCAF"), document);
  const auto shape_tool = XCAFDoc_DocumentTool::ShapeTool(document->Main());
  const auto color_tool = XCAFDoc_DocumentTool::ColorTool(document->Main());
  XCAFDoc_ShapeTool::SetAutoNaming(Standard_False);

  if (empty) {
    const auto edge = shape_tool->AddShape(
        BRepBuilderAPI_MakeEdge(gp_Pnt(0.0, 0.0, 0.0),
                                gp_Pnt(10.0, 0.0, 0.0))
            .Shape(),
        Standard_False);
    TDataStd_Name::Set(edge, name("No renderable faces"));
  } else {
    const auto block = shape_tool->AddShape(
        BRepPrimAPI_MakeBox(20.0, 10.0, 5.0).Shape(), Standard_False);
    TDataStd_Name::Set(block, name("Forge Block"));
    color_tool->SetColor(block,
                         Quantity_Color(0.82, 0.16, 0.08, Quantity_TOC_sRGB),
                         XCAFDoc_ColorGen);

    const auto axle = shape_tool->AddShape(
        BRepPrimAPI_MakeCylinder(3.0, 16.0).Shape(), Standard_False);
    TDataStd_Name::Set(axle, name("Axle"));
    color_tool->SetColor(axle,
                         Quantity_Color(0.08, 0.25, 0.82, Quantity_TOC_sRGB),
                         XCAFDoc_ColorGen);

    const auto assembly = shape_tool->NewShape();
    TDataStd_Name::Set(assembly, name("VÖLUND Test Assembly"));

    const auto block_instance =
        shape_tool->AddComponent(assembly, block, TopLoc_Location());
    TDataStd_Name::Set(block_instance, name("Base Instance"));

    gp_Trsf translated_block;
    translated_block.SetTranslation(gp_Vec(30.0, 0.0, 0.0));
    const auto block_copy = shape_tool->AddComponent(
        assembly, block, TopLoc_Location(translated_block));
    TDataStd_Name::Set(block_copy, name("Base Copy"));

    gp_Trsf translated_axle;
    translated_axle.SetTranslation(gp_Vec(10.0, 5.0, 5.0));
    const auto axle_instance = shape_tool->AddComponent(
        assembly, axle, TopLoc_Location(translated_axle));
    TDataStd_Name::Set(axle_instance, name("Axle Instance"));
    shape_tool->UpdateAssemblies();
  }

  const auto extension = output.extension().string();
  bool written = false;
  if (extension == ".step" || extension == ".stp") {
    STEPCAFControl_Writer writer;
    writer.SetColorMode(Standard_True);
    writer.SetNameMode(Standard_True);
    written = writer.Transfer(document, STEPControl_AsIs) &&
              writer.Write(output.string().c_str()) == IFSelect_RetDone;
  } else if (extension == ".iges" || extension == ".igs") {
    IGESCAFControl_Writer writer;
    writer.SetColorMode(Standard_True);
    writer.SetNameMode(Standard_True);
    written = writer.Perform(document, output.string().c_str());
  } else if (extension == ".brep") {
    written = BRepTools::Write(shape_tool->GetOneShape(), output.string().c_str());
  }
  if (!written) {
    application->Close(document);
    std::cerr << "failed to write CAD fixture: " << output.string() << '\n';
    return EXIT_FAILURE;
  }

  application->Close(document);
  std::cout << output.string() << '\n';
  return EXIT_SUCCESS;
}
