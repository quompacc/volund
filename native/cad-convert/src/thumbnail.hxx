#pragma once

#include <TDocStd_Document.hxx>

#include <filesystem>

void write_thumbnail(const Handle(TDocStd_Document)& document,
                     const std::filesystem::path& path);
