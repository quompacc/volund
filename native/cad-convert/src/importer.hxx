#pragma once

#include <TDocStd_Document.hxx>

#include <filesystem>
#include <string>
#include <string_view>

enum class SourceFormat { step, iges, brep, stl, three_mf, obj, ply, gltf, glb };

std::string_view format_name(SourceFormat format);
SourceFormat resolve_source_format(const std::filesystem::path& path,
                                   std::string_view requested_format);
Handle(TDocStd_Document) import_document(const std::filesystem::path& path,
                                         SourceFormat format);
