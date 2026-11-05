#include "three_mf.hxx"

#include <zlib.h>

#include <cctype>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <optional>
#include <stdexcept>
#include <utility>

namespace volund::three_mf {
namespace {
constexpr std::uintmax_t max_package_bytes = 1ULL << 30;   // 1 GiB package
constexpr std::uint64_t max_entry_bytes = 1ULL << 30;      // 1 GiB per part
constexpr std::size_t max_triangles = 30'000'000;
constexpr int max_depth = 32;

std::uint32_t u32(const std::string& d, std::size_t at) {
  if (at + 4 > d.size()) throw std::runtime_error("3MF package is truncated");
  return static_cast<std::uint32_t>(static_cast<unsigned char>(d[at])) |
         static_cast<std::uint32_t>(static_cast<unsigned char>(d[at + 1])) << 8U |
         static_cast<std::uint32_t>(static_cast<unsigned char>(d[at + 2])) << 16U |
         static_cast<std::uint32_t>(static_cast<unsigned char>(d[at + 3])) << 24U;
}
std::uint64_t u64(const std::string& d, std::size_t at) {
  return static_cast<std::uint64_t>(u32(d, at)) | static_cast<std::uint64_t>(u32(d, at + 4)) << 32U;
}
std::uint16_t u16(const std::string& d, std::size_t at) {
  if (at + 2 > d.size()) throw std::runtime_error("3MF package is truncated");
  return static_cast<std::uint16_t>(static_cast<unsigned char>(d[at]) |
                                    static_cast<unsigned char>(d[at + 1]) << 8U);
}

// Read-only ZIP container: stored and deflated entries, no ZIP64.
class Zip {
 public:
  explicit Zip(const std::filesystem::path& path) {
    if (std::filesystem::file_size(path) > max_package_bytes) throw std::runtime_error("3MF package is too large");
    std::ifstream input(path, std::ios::binary);
    data_.assign(std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>());
    if (data_.size() < 22) throw std::runtime_error("3MF package is not a ZIP archive");
    std::size_t eocd = std::string::npos;
    const std::size_t lowest = data_.size() > 65557 ? data_.size() - 65557 : 0;
    for (std::size_t at = data_.size() - 22;; --at) {
      if (u32(data_, at) == 0x06054b50U) { eocd = at; break; }
      if (at == lowest) break;
    }
    if (eocd == std::string::npos) throw std::runtime_error("3MF package has no ZIP directory");
    std::uint64_t count = u16(data_, eocd + 10);
    std::uint64_t directory = u32(data_, eocd + 16);
    if ((count == 0xffffU || directory == 0xffffffffU) && eocd >= 20 && u32(data_, eocd - 20) == 0x07064b50U) {
      const auto record = static_cast<std::size_t>(u64(data_, eocd - 12));  // ZIP64 end of central directory
      if (u32(data_, record) != 0x06064b50U) throw std::runtime_error("3MF ZIP64 directory is corrupt");
      count = u64(data_, record + 32);
      directory = u64(data_, record + 48);
    }
    auto at = static_cast<std::size_t>(directory);
    for (std::uint64_t index = 0; index < count; ++index) {
      if (u32(data_, at) != 0x02014b50U) throw std::runtime_error("3MF ZIP directory is corrupt");
      const auto name_length = u16(data_, at + 28);
      const auto extra_length = u16(data_, at + 30);
      Entry entry{u16(data_, at + 10), u32(data_, at + 20), u32(data_, at + 24), u32(data_, at + 42)};
      // ZIP64 extra field: 64-bit values replace the 32-bit fields set to 0xffffffff, in this order.
      for (std::size_t extra = at + 46 + name_length; extra + 4 <= at + 46 + name_length + extra_length;) {
        const auto id = u16(data_, extra);
        const auto length = u16(data_, extra + 2);
        if (id == 0x0001U) {
          std::size_t value = extra + 4;
          if (entry.size == 0xffffffffU) { entry.size = u64(data_, value); value += 8; }
          if (entry.compressed == 0xffffffffU) { entry.compressed = u64(data_, value); value += 8; }
          if (entry.offset == 0xffffffffU) { entry.offset = u64(data_, value); }
        }
        extra += 4U + length;
      }
      if (at + 46 + name_length > data_.size()) throw std::runtime_error("3MF package is truncated");
      entries_[data_.substr(at + 46, name_length)] = entry;
      at += 46U + name_length + extra_length + u16(data_, at + 32);
    }
  }

  [[nodiscard]] std::optional<std::string> read(const std::string& name) const {
    const auto found = entries_.find(name);
    if (found == entries_.end()) return std::nullopt;
    const auto& entry = found->second;
    if (entry.size > max_entry_bytes || entry.compressed > max_package_bytes) throw std::runtime_error("3MF part is too large");
    const auto local = static_cast<std::size_t>(entry.offset);
    if (u32(data_, local) != 0x04034b50U) throw std::runtime_error("3MF ZIP entry is corrupt");
    const std::size_t start = local + 30U + u16(data_, local + 26) + u16(data_, local + 28);
    if (start + entry.compressed > data_.size()) throw std::runtime_error("3MF package is truncated");
    if (entry.method == 0) return data_.substr(start, static_cast<std::size_t>(entry.compressed));
    if (entry.method != 8) throw std::runtime_error("3MF part uses an unsupported compression method");
    std::string out(static_cast<std::size_t>(entry.size), '\0');
    z_stream stream{};
    if (inflateInit2(&stream, -MAX_WBITS) != Z_OK) throw std::runtime_error("cannot initialise inflate");
    stream.next_in = reinterpret_cast<Bytef*>(const_cast<char*>(data_.data() + start));
    stream.avail_in = static_cast<uInt>(entry.compressed);
    stream.next_out = reinterpret_cast<Bytef*>(out.data());
    stream.avail_out = static_cast<uInt>(entry.size);
    const int result = inflate(&stream, Z_FINISH);
    inflateEnd(&stream);
    if (result != Z_STREAM_END || stream.total_out != entry.size) throw std::runtime_error("3MF part cannot be decompressed");
    return out;
  }

 private:
  struct Entry { std::uint16_t method; std::uint64_t compressed, size, offset; };
  std::string data_;
  std::map<std::string, Entry> entries_;
};

struct Tag {
  std::string name;  // namespace prefix removed
  bool closing = false, self_closing = false;
  std::map<std::string, std::string> attributes;
};

std::string decode_entities(std::string value) {
  static const std::array<std::pair<const char*, char>, 5> entities{
      {{"&amp;", '&'}, {"&lt;", '<'}, {"&gt;", '>'}, {"&quot;", '"'}, {"&apos;", '\''}}};
  for (const auto& [entity, ch] : entities) {
    for (auto at = value.find(entity); at != std::string::npos; at = value.find(entity, at + 1)) {
      value.replace(at, std::strlen(entity), 1, ch);
    }
  }
  return value;
}

// Calls visit(tag) for every element tag in document order.
template <typename Visit>
void scan_tags(const std::string& xml, Visit visit) {
  std::size_t at = 0;
  while ((at = xml.find('<', at)) != std::string::npos) {
    if (xml.compare(at, 4, "<!--") == 0) { at = xml.find("-->", at); if (at == std::string::npos) return; continue; }
    if (at + 1 < xml.size() && (xml[at + 1] == '?' || xml[at + 1] == '!')) { at = xml.find('>', at); if (at == std::string::npos) return; continue; }
    Tag tag;
    std::size_t p = at + 1;
    if (p < xml.size() && xml[p] == '/') { tag.closing = true; ++p; }
    const auto name_end = xml.find_first_of(" \t\r\n/>", p);
    if (name_end == std::string::npos) return;
    tag.name = xml.substr(p, name_end - p);
    if (const auto colon = tag.name.find(':'); colon != std::string::npos) tag.name.erase(0, colon + 1);
    p = name_end;
    while (p < xml.size() && xml[p] != '>') {
      if (xml[p] == '/') { tag.self_closing = true; ++p; continue; }
      if (std::isspace(static_cast<unsigned char>(xml[p])) != 0) { ++p; continue; }
      const auto eq = xml.find('=', p);
      if (eq == std::string::npos) return;
      std::string key = xml.substr(p, eq - p);
      while (!key.empty() && std::isspace(static_cast<unsigned char>(key.back())) != 0) key.pop_back();
      const auto quote_at = xml.find_first_of("\"'", eq);
      if (quote_at == std::string::npos) return;
      const auto close = xml.find(xml[quote_at], quote_at + 1);
      if (close == std::string::npos) return;
      tag.attributes[key] = decode_entities(xml.substr(quote_at + 1, close - quote_at - 1));
      p = close + 1;
    }
    visit(tag);
    at = p;
  }
}

double number(const Tag& tag, const char* key) {
  const auto found = tag.attributes.find(key);
  if (found == tag.attributes.end()) throw std::runtime_error(std::string("3MF ") + tag.name + " lacks " + key);
  return std::strtod(found->second.c_str(), nullptr);
}

Transform transform_of(const Tag& tag) {
  const auto found = tag.attributes.find("transform");
  if (found == tag.attributes.end()) return identity;
  Transform t{};
  const char* cursor = found->second.c_str();
  for (auto& value : t) {
    char* end = nullptr;
    value = std::strtod(cursor, &end);
    if (end == cursor) throw std::runtime_error("3MF transform is malformed");
    cursor = end;
  }
  return t;
}

struct Reference { std::string file; int object; Transform transform; };
struct RawObject { std::string name; std::vector<Vertex> vertices; std::vector<std::array<int, 3>> triangles; std::vector<Reference> components; };
struct Model { std::map<int, RawObject> objects; std::vector<Reference> build; };

std::string part_name(std::string path) {
  if (!path.empty() && path.front() == '/') path.erase(0, 1);
  return path;
}

Model parse_model(const std::string& xml, const std::string& file) {
  Model model;
  RawObject* current = nullptr;
  bool in_build = false;
  scan_tags(xml, [&](const Tag& tag) {
    const auto attribute = [&](const char* key) { const auto f = tag.attributes.find(key); return f == tag.attributes.end() ? std::string() : f->second; };
    if (tag.name == "object") {
      if (tag.closing) { current = nullptr; return; }
      current = &model.objects[static_cast<int>(number(tag, "id"))];
      current->name = attribute("name");
      if (tag.self_closing) current = nullptr;
    } else if (tag.name == "build") {
      in_build = !tag.closing && !tag.self_closing;
    } else if (tag.closing) {
      return;
    } else if (tag.name == "vertex" && current != nullptr) {
      current->vertices.push_back({number(tag, "x"), number(tag, "y"), number(tag, "z")});
    } else if (tag.name == "triangle" && current != nullptr) {
      current->triangles.push_back({static_cast<int>(number(tag, "v1")), static_cast<int>(number(tag, "v2")), static_cast<int>(number(tag, "v3"))});
    } else if (tag.name == "component" && current != nullptr) {
      const auto path = attribute("p:path");
      current->components.push_back({path.empty() ? file : part_name(path), static_cast<int>(number(tag, "objectid")), transform_of(tag)});
    } else if (tag.name == "item" && in_build) {
      const auto path = attribute("p:path");
      model.build.push_back({path.empty() ? file : part_name(path), static_cast<int>(number(tag, "objectid")), transform_of(tag)});
    }
  });
  return model;
}

std::string percent_decoded(const std::string& value) {
  std::string out;
  for (std::size_t i = 0; i < value.size(); ++i) {
    if (value[i] == '%' && i + 2 < value.size() && std::isxdigit(static_cast<unsigned char>(value[i + 1])) != 0 &&
        std::isxdigit(static_cast<unsigned char>(value[i + 2])) != 0) {
      out.push_back(static_cast<char>(std::stoi(value.substr(i + 1, 2), nullptr, 16)));
      i += 2;
    } else {
      out.push_back(value[i]);
    }
  }
  return out;
}

class Package {
 public:
  explicit Package(const std::filesystem::path& path) : zip_(path) {}

  const Model& model(const std::string& file) {
    if (const auto found = models_.find(file); found != models_.end()) return found->second;
    auto xml = zip_.read(file);
    if (!xml) xml = zip_.read(percent_decoded(file));
    if (!xml) throw std::runtime_error("3MF references a missing part: " + file);
    return models_.emplace(file, parse_model(*xml, file)).first->second;
  }

  [[nodiscard]] std::string root_part() const {
    std::string root = "3D/3dmodel.model";
    if (const auto rels = zip_.read("_rels/.rels")) {
      scan_tags(*rels, [&](const Tag& tag) {
        const auto target = tag.attributes.find("Target");
        const auto type = tag.attributes.find("Type");
        if (tag.name == "Relationship" && target != tag.attributes.end() && type != tag.attributes.end() &&
            type->second.find("3dmodel") != std::string::npos) root = part_name(target->second);
      });
    }
    return root;
  }

  // Index of (file, object) in scene.objects, adding it and its components once.
  std::size_t resolve(const std::string& file, int id, int depth, Scene& scene) {
    const auto key = std::make_pair(file, id);
    if (const auto found = index_.find(key); found != index_.end()) {
      if (found->second == pending) throw std::runtime_error("3MF components form a cycle");
      return found->second;
    }
    if (depth > max_depth) throw std::runtime_error("3MF components nest too deeply");
    const auto& part = model(file);
    const auto found = part.objects.find(id);
    if (found == part.objects.end()) throw std::runtime_error("3MF references a missing object");
    const auto& source = found->second;
    for (const auto& triangle : source.triangles) {
      for (const int index : triangle) {
        if (index < 0 || static_cast<std::size_t>(index) >= source.vertices.size()) throw std::runtime_error("3MF triangle references a missing vertex");
      }
    }
    triangles_ += source.triangles.size();
    if (triangles_ > max_triangles) throw std::runtime_error("3MF model has too many triangles");
    index_[key] = pending;
    Object object{source.name, source.vertices, source.triangles, {}};
    for (const auto& component : source.components) {
      object.components.push_back({resolve(component.file, component.object, depth + 1, scene), component.transform});
    }
    scene.objects.push_back(std::move(object));
    return index_[key] = scene.objects.size() - 1;
  }

 private:
  static constexpr std::size_t pending = static_cast<std::size_t>(-1);
  Zip zip_;
  std::map<std::string, Model> models_;
  std::map<std::pair<std::string, int>, std::size_t> index_;
  std::size_t triangles_ = 0;
};

void flatten_into(const Scene& scene, std::size_t index, const Transform& world, std::vector<Mesh>& out) {
  const auto& object = scene.objects[index];
  if (!object.triangles.empty()) {
    Mesh mesh{object.name, {}, object.triangles};
    mesh.vertices.reserve(object.vertices.size());
    for (const auto& vertex : object.vertices) mesh.vertices.push_back(apply(world, vertex));
    out.push_back(std::move(mesh));
  }
  for (const auto& component : object.components) flatten_into(scene, component.object, compose(component.transform, world), out);
}
}  // namespace

Vertex apply(const Transform& t, const Vertex& p) {
  return {p[0] * t[0] + p[1] * t[3] + p[2] * t[6] + t[9], p[0] * t[1] + p[1] * t[4] + p[2] * t[7] + t[10],
          p[0] * t[2] + p[1] * t[5] + p[2] * t[8] + t[11]};
}

Transform compose(const Transform& first, const Transform& second) {
  Transform r{};
  for (std::size_t row = 0; row < 4; ++row) {
    for (std::size_t col = 0; col < 3; ++col) {
      double value = row == 3 ? second[9 + col] : 0.0;
      for (std::size_t k = 0; k < 3; ++k) value += first[3 * row + k] * second[3 * k + col];
      r[3 * row + col] = value;
    }
  }
  return r;
}

bool is_rigid(const Transform& t) {
  for (std::size_t a = 0; a < 3; ++a) {
    for (std::size_t b = 0; b < 3; ++b) {
      const double d = t[3 * a] * t[3 * b] + t[3 * a + 1] * t[3 * b + 1] + t[3 * a + 2] * t[3 * b + 2];
      if (std::abs(d - (a == b ? 1.0 : 0.0)) > 1e-6) return false;
    }
  }
  const double det = t[0] * (t[4] * t[8] - t[5] * t[7]) - t[1] * (t[3] * t[8] - t[5] * t[6]) + t[2] * (t[3] * t[7] - t[4] * t[6]);
  return det > 0.0;
}

std::vector<Mesh> flatten(const Scene& scene, std::size_t object, const Transform& transform) {
  std::vector<Mesh> out;
  flatten_into(scene, object, transform, out);
  return out;
}

Scene read(const std::filesystem::path& path) {
  Package package(path);
  const auto root = package.root_part();
  const auto& model = package.model(root);
  std::vector<Reference> items = model.build;
  if (items.empty()) {
    for (const auto& [id, object] : model.objects) items.push_back({root, id, identity});
  }
  Scene scene;
  for (const auto& item : items) scene.build.push_back({package.resolve(item.file, item.object, 0, scene), item.transform});
  bool has_triangles = false;
  for (const auto& object : scene.objects) has_triangles = has_triangles || !object.triangles.empty();
  if (!has_triangles) throw std::runtime_error("3MF package contains no triangles");
  return scene;
}

}  // namespace volund::three_mf
