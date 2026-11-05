// Unit tests for the 3MF reader: core packages, production-extension parts,
// transforms, compression and failure cases.
#include "../src/three_mf.hxx"

#include <zlib.h>

#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace fs = std::filesystem;

namespace {
int failures = 0;

void check(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "FAIL: " << message << '\n';
    ++failures;
  }
}

void put16(std::string& out, unsigned value) {
  out.push_back(static_cast<char>(value & 0xffU));
  out.push_back(static_cast<char>((value >> 8U) & 0xffU));
}
void put32(std::string& out, unsigned long value) {
  put16(out, static_cast<unsigned>(value & 0xffffU));
  put16(out, static_cast<unsigned>((value >> 16U) & 0xffffU));
}
void put64(std::string& out, unsigned long long value) {
  put32(out, static_cast<unsigned long>(value & 0xffffffffULL));
  put32(out, static_cast<unsigned long>(value >> 32U));
}

std::string raw_deflate(const std::string& input) {
  z_stream stream{};
  deflateInit2(&stream, Z_BEST_COMPRESSION, Z_DEFLATED, -MAX_WBITS, 8, Z_DEFAULT_STRATEGY);
  std::string out(deflateBound(&stream, static_cast<uLong>(input.size())), '\0');
  stream.next_in = reinterpret_cast<Bytef*>(const_cast<char*>(input.data()));
  stream.avail_in = static_cast<uInt>(input.size());
  stream.next_out = reinterpret_cast<Bytef*>(out.data());
  stream.avail_out = static_cast<uInt>(out.size());
  deflate(&stream, Z_FINISH);
  out.resize(stream.total_out);
  deflateEnd(&stream);
  return out;
}

// Write a ZIP with the given entries; `deflated` compresses every entry and
// `zip64` stores sizes, offsets and the directory location in ZIP64 records.
fs::path write_zip(const fs::path& path, const std::vector<std::pair<std::string, std::string>>& entries, bool deflated,
                   bool zip64 = false) {
  std::string body, directory;
  for (const auto& [name, content] : entries) {
    const auto crc = crc32(0L, reinterpret_cast<const Bytef*>(content.data()), static_cast<uInt>(content.size()));
    const std::string stored = deflated ? raw_deflate(content) : content;
    const auto offset = body.size();
    put32(body, 0x04034b50UL); put16(body, 20); put16(body, 0); put16(body, deflated ? 8 : 0); put32(body, 0);
    put32(body, crc); put32(body, stored.size()); put32(body, content.size()); put16(body, static_cast<unsigned>(name.size())); put16(body, 0);
    body += name + stored;
    put32(directory, 0x02014b50UL); put16(directory, 45); put16(directory, 45); put16(directory, 0); put16(directory, deflated ? 8 : 0);
    put32(directory, 0); put32(directory, crc);
    put32(directory, zip64 ? 0xffffffffUL : stored.size()); put32(directory, zip64 ? 0xffffffffUL : content.size());
    put16(directory, static_cast<unsigned>(name.size())); put16(directory, zip64 ? 28 : 0); put16(directory, 0); put16(directory, 0); put16(directory, 0);
    put32(directory, 0); put32(directory, zip64 ? 0xffffffffUL : offset);
    directory += name;
    if (zip64) { put16(directory, 1); put16(directory, 24); put64(directory, content.size()); put64(directory, stored.size()); put64(directory, offset); }
  }
  std::string zip = body + directory;
  if (zip64) {
    const auto record = zip.size();
    put32(zip, 0x06064b50UL); put64(zip, 44); put16(zip, 45); put16(zip, 45); put32(zip, 0); put32(zip, 0);
    put64(zip, entries.size()); put64(zip, entries.size()); put64(zip, directory.size()); put64(zip, body.size());
    put32(zip, 0x07064b50UL); put32(zip, 0); put64(zip, record); put32(zip, 1);
  }
  put32(zip, 0x06054b50UL); put16(zip, 0); put16(zip, 0);
  put16(zip, zip64 ? 0xffffU : static_cast<unsigned>(entries.size())); put16(zip, zip64 ? 0xffffU : static_cast<unsigned>(entries.size()));
  put32(zip, zip64 ? 0xffffffffUL : directory.size()); put32(zip, zip64 ? 0xffffffffUL : body.size()); put16(zip, 0);
  std::ofstream(path, std::ios::binary) << zip;
  return path;
}

const std::string rels =
    "<?xml version=\"1.0\"?><Relationships><Relationship Target=\"/3D/3dmodel.model\" Id=\"r1\" "
    "Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";
const std::string triangle_mesh =
    "<mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/>"
    "<vertex x=\"0\" y=\"1\" z=\"0\"/></vertices><triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh>";

bool near(double a, double b) { return std::abs(a - b) < 1e-9; }

template <typename F>
bool throws(F call) {
  try { call(); } catch (const std::runtime_error&) { return true; }
  return false;
}

void reads_core_package_with_build_transform(const fs::path& dir) {
  const auto model = "<?xml version=\"1.0\"?><model unit=\"millimeter\"><!-- comment <vertex x=\"9\"/> -->"
                     "<resources><object id=\"1\" name=\"Halter &amp; Deckel\" type=\"model\">" + triangle_mesh +
                     "</object></resources><build><item objectid=\"1\" transform=\"1 0 0 0 1 0 0 0 1 10 0 0\"/></build></model>";
  const auto scene = volund::three_mf::read(write_zip(dir / "core.3mf", {{"_rels/.rels", rels}, {"3D/3dmodel.model", model}}, false));
  check(scene.objects.size() == 1 && scene.build.size() == 1, "core package yields one object and one build item");
  const auto& object = scene.objects[scene.build[0].object];
  check(object.name == "Halter & Deckel", "object name with entity is decoded");
  check(object.triangles.size() == 1 && object.vertices.size() == 3, "triangle and vertices are read, comments ignored");
  const auto meshes = volund::three_mf::flatten(scene, scene.build[0].object, scene.build[0].transform);
  check(meshes.size() == 1 && near(meshes[0].vertices[1][0], 11.0), "build transform translates vertices");
}

void reads_production_parts_with_nested_transforms(const fs::path& dir) {
  const auto root = "<model xmlns:p=\"http://schemas.microsoft.com/3dmanufacturing/production/2015/06\"><resources>"
                    "<object id=\"2\" type=\"model\"><components><component p:path=\"/3D/Objects/part%20a.model\" objectid=\"1\" "
                    "transform=\"1 0 0 0 1 0 0 0 1 0 0 5\"/></components></object></resources>"
                    "<build><item objectid=\"2\" transform=\"2 0 0 0 2 0 0 0 2 0 0 0\"/><item objectid=\"2\"/></build></model>";
  const auto part = "<model><resources><object id=\"1\" name=\"Body\">" + triangle_mesh + "</object></resources><build/></model>";
  const auto scene = volund::three_mf::read(write_zip(dir / "slicer.3mf",
      {{"_rels/.rels", rels}, {"3D/3dmodel.model", root}, {"3D/Objects/part%20a.model", part}}, true));
  check(scene.objects.size() == 2, "the part object is shared, not copied per build item");
  check(scene.build.size() == 2 && scene.build[0].object == scene.build[1].object, "both build items reference one object");
  const auto& wrapper = scene.objects[scene.build[0].object];
  check(wrapper.components.size() == 1 && scene.objects[wrapper.components[0].object].name == "Body", "component resolves into the part file");
  const auto first = volund::three_mf::flatten(scene, scene.build[0].object, scene.build[0].transform);
  check(first.size() == 1 && near(first[0].vertices[0][2], 10.0), "component then item transform: (0,0,0)+5z, scaled x2");
  check(near(first[0].vertices[1][0], 2.0), "item scale applies to component vertices");
  const auto second = volund::three_mf::flatten(scene, scene.build[1].object, scene.build[1].transform);
  check(near(second[0].vertices[0][2], 5.0), "second item without transform keeps the component offset");
}

void reads_zip64_packages(const fs::path& dir) {
  const auto model = "<model><resources><object id=\"1\" name=\"Z64\">" + triangle_mesh +
                     "</object></resources><build><item objectid=\"1\"/></build></model>";
  const auto scene = volund::three_mf::read(write_zip(dir / "zip64.3mf", {{"_rels/.rels", rels}, {"3D/3dmodel.model", model}}, true, true));
  check(scene.objects.size() == 1 && scene.objects[0].name == "Z64" && scene.objects[0].triangles.size() == 1,
        "ZIP64 sizes, offsets and directory are read");
}

void classifies_rigid_transforms() {
  using volund::three_mf::is_rigid;
  check(is_rigid(volund::three_mf::identity), "identity is rigid");
  check(is_rigid({0, 1, 0, -1, 0, 0, 0, 0, 1, 5, 6, 7}), "rotation plus translation is rigid");
  check(!is_rigid({2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0}), "scale is not rigid");
  check(!is_rigid({-1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0}), "mirror is not rigid");
  const auto both = volund::three_mf::compose({1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0}, {0, 1, 0, -1, 0, 0, 0, 0, 1, 0, 0, 0});
  const auto moved = volund::three_mf::apply(both, {0, 0, 0});
  check(near(moved[0], 0.0) && near(moved[1], 1.0), "compose translates first, then rotates");
}

void rejects_broken_packages(const fs::path& dir) {
  const auto empty = "<model><resources><object id=\"1\"><mesh><vertices/><triangles/></mesh></object></resources>"
                     "<build><item objectid=\"1\"/></build></model>";
  check(throws([&] { volund::three_mf::read(write_zip(dir / "empty.3mf", {{"3D/3dmodel.model", empty}}, false)); }),
        "package without triangles is rejected");
  const auto missing = "<model><resources><object id=\"2\"><components><component p:path=\"/3D/Objects/gone.model\" objectid=\"1\"/>"
                       "</components></object></resources><build><item objectid=\"2\"/></build></model>";
  check(throws([&] { volund::three_mf::read(write_zip(dir / "missing.3mf", {{"3D/3dmodel.model", missing}}, false)); }),
        "missing component part is rejected");
  const auto bad_index = "<model><resources><object id=\"1\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/></vertices>"
                         "<triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object></resources>"
                         "<build><item objectid=\"1\"/></build></model>";
  check(throws([&] { volund::three_mf::read(write_zip(dir / "index.3mf", {{"3D/3dmodel.model", bad_index}}, false)); }),
        "triangle pointing past the vertex list is rejected");
  const auto cycle = "<model><resources><object id=\"1\"><components><component objectid=\"1\"/></components></object>"
                     "</resources><build><item objectid=\"1\"/></build></model>";
  check(throws([&] { volund::three_mf::read(write_zip(dir / "cycle.3mf", {{"3D/3dmodel.model", cycle}}, false)); }),
        "self-referencing component is rejected");
  std::ofstream(dir / "not-a-zip.3mf") << "solid ascii stl pretending to be 3mf";
  check(throws([&] { volund::three_mf::read(dir / "not-a-zip.3mf"); }), "non-ZIP file is rejected");
}
}  // namespace

int main(int argc, char** argv) {
  const fs::path dir = argc > 1 ? fs::path(argv[1]) : fs::temp_directory_path() / "volund-three-mf-test";
  fs::create_directories(dir);
  reads_core_package_with_build_transform(dir);
  reads_production_parts_with_nested_transforms(dir);
  classifies_rigid_transforms();
  reads_zip64_packages(dir);
  rejects_broken_packages(dir);
  if (failures != 0) return EXIT_FAILURE;
  std::cout << "3MF reader tests passed\n";
  return EXIT_SUCCESS;
}
