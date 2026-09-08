if(NOT DEFINED GENERATOR OR NOT DEFINED CONVERTER OR NOT DEFINED ASSIMP OR
   NOT DEFINED OUTPUT_DIR)
  message(FATAL_ERROR "GENERATOR, CONVERTER, ASSIMP and OUTPUT_DIR are required")
endif()

file(REMOVE_RECURSE "${OUTPUT_DIR}")
file(MAKE_DIRECTORY "${OUTPUT_DIR}")

foreach(FORMAT step iges brep)
  execute_process(
    COMMAND "${GENERATOR}" "${OUTPUT_DIR}/model.${FORMAT}"
    RESULT_VARIABLE RESULT
    ERROR_VARIABLE ERROR
  )
  if(NOT RESULT EQUAL 0)
    message(FATAL_ERROR "could not generate ${FORMAT}: ${ERROR}")
  endif()
endforeach()

execute_process(
  COMMAND "${CONVERTER}" convert --input "${OUTPUT_DIR}/model.step"
          --output "${OUTPUT_DIR}/seed"
  RESULT_VARIABLE RESULT
  ERROR_VARIABLE ERROR
)
if(NOT RESULT EQUAL 0)
  message(FATAL_ERROR "could not generate mesh seed: ${ERROR}")
endif()

foreach(SPEC "stl:stlb" "obj:obj" "ply:plyb" "gltf:gltf2")
  string(REPLACE ":" ";" PARTS "${SPEC}")
  list(GET PARTS 0 EXTENSION)
  list(GET PARTS 1 EXPORT_ID)
  execute_process(
    COMMAND "${ASSIMP}" export "${OUTPUT_DIR}/seed/preview.glb"
            "${OUTPUT_DIR}/model.${EXTENSION}" -f "${EXPORT_ID}"
    RESULT_VARIABLE RESULT
    ERROR_VARIABLE ERROR
  )
  if(NOT RESULT EQUAL 0)
    message(FATAL_ERROR "could not generate ${EXTENSION}: ${ERROR}")
  endif()
endforeach()
file(COPY_FILE "${OUTPUT_DIR}/seed/preview.glb" "${OUTPUT_DIR}/model.glb")

# A small native 3MF component fixture verifies instancing and transforms. A
# GLB-to-3MF conversion would flatten the scene and could not cover that case.
set(THREE_MF_DIR "${OUTPUT_DIR}/three-mf-package")
file(MAKE_DIRECTORY "${THREE_MF_DIR}/_rels" "${THREE_MF_DIR}/3D")
file(WRITE "${THREE_MF_DIR}/[Content_Types].xml"
  "<?xml version=\"1.0\" encoding=\"UTF-8\"?>"
  "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">"
  "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>"
  "<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>"
  "</Types>")
file(WRITE "${THREE_MF_DIR}/_rels/.rels"
  "<?xml version=\"1.0\" encoding=\"UTF-8\"?>"
  "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">"
  "<Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>"
  "</Relationships>")
file(WRITE "${THREE_MF_DIR}/3D/3dmodel.model"
  "<?xml version=\"1.0\" encoding=\"UTF-8\"?>"
  "<model unit=\"millimeter\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">"
  "<resources>"
  "<object id=\"1\" type=\"model\" name=\"Forge Cube\"><mesh><vertices>"
  "<vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"10\" y=\"0\" z=\"0\"/>"
  "<vertex x=\"10\" y=\"10\" z=\"0\"/><vertex x=\"0\" y=\"10\" z=\"0\"/>"
  "<vertex x=\"0\" y=\"0\" z=\"10\"/><vertex x=\"10\" y=\"0\" z=\"10\"/>"
  "<vertex x=\"10\" y=\"10\" z=\"10\"/><vertex x=\"0\" y=\"10\" z=\"10\"/>"
  "</vertices><triangles>"
  "<triangle v1=\"0\" v2=\"2\" v3=\"1\"/><triangle v1=\"0\" v2=\"3\" v3=\"2\"/>"
  "<triangle v1=\"4\" v2=\"5\" v3=\"6\"/><triangle v1=\"4\" v2=\"6\" v3=\"7\"/>"
  "<triangle v1=\"0\" v2=\"1\" v3=\"5\"/><triangle v1=\"0\" v2=\"5\" v3=\"4\"/>"
  "<triangle v1=\"1\" v2=\"2\" v3=\"6\"/><triangle v1=\"1\" v2=\"6\" v3=\"5\"/>"
  "<triangle v1=\"2\" v2=\"3\" v3=\"7\"/><triangle v1=\"2\" v2=\"7\" v3=\"6\"/>"
  "<triangle v1=\"3\" v2=\"0\" v3=\"4\"/><triangle v1=\"3\" v2=\"4\" v3=\"7\"/>"
  "</triangles></mesh></object>"
  "<object id=\"2\" type=\"model\" name=\"Forged Pair\"><components>"
  "<component objectid=\"1\"/><component objectid=\"1\" transform=\"1 0 0 0 1 0 0 0 1 30 0 0\"/>"
  "</components></object></resources><build><item objectid=\"2\"/></build></model>")
execute_process(
  COMMAND "${CMAKE_COMMAND}" -E tar cf "${OUTPUT_DIR}/model.3mf" --format=zip
          "[Content_Types].xml" "_rels/.rels" "3D/3dmodel.model"
  WORKING_DIRECTORY "${THREE_MF_DIR}"
  RESULT_VARIABLE RESULT
  ERROR_VARIABLE ERROR
)
if(NOT RESULT EQUAL 0)
  message(FATAL_ERROR "could not package native 3MF fixture: ${ERROR}")
endif()

foreach(FORMAT step iges brep stl obj ply gltf glb 3mf)
  set(ARTIFACTS "${OUTPUT_DIR}/out-${FORMAT}")
  execute_process(
    COMMAND "${CONVERTER}" convert --input "${OUTPUT_DIR}/model.${FORMAT}"
            --output "${ARTIFACTS}"
    RESULT_VARIABLE RESULT
    ERROR_VARIABLE ERROR
  )
  if(NOT RESULT EQUAL 0)
    message(FATAL_ERROR "${FORMAT} conversion failed: ${ERROR}")
  endif()
  foreach(ARTIFACT preview.glb thumbnail.png assembly.json diagnostics.json result.json)
    if(NOT EXISTS "${ARTIFACTS}/${ARTIFACT}")
      message(FATAL_ERROR "${FORMAT} missed ${ARTIFACT}")
    endif()
  endforeach()
  file(READ "${ARTIFACTS}/result.json" RESULT_JSON)
  string(JSON REPORTED_FORMAT GET "${RESULT_JSON}" source format)
  string(JSON TRIANGLE_COUNT GET "${RESULT_JSON}" preview triangleCount)
  if(NOT REPORTED_FORMAT STREQUAL FORMAT)
    message(FATAL_ERROR "${FORMAT} reported itself as ${REPORTED_FORMAT}")
  endif()
  if(TRIANGLE_COUNT LESS 2)
    message(FATAL_ERROR "${FORMAT} lost its mesh: ${TRIANGLE_COUNT} triangles")
  endif()
  file(READ "${ARTIFACTS}/preview.glb" GLB_MAGIC OFFSET 0 LIMIT 4 HEX)
  if(NOT GLB_MAGIC STREQUAL "676c5446")
    message(FATAL_ERROR "${FORMAT} produced no valid binary glTF preview")
  endif()
endforeach()

file(READ "${OUTPUT_DIR}/out-3mf/assembly.json" THREE_MF_ASSEMBLY)
string(FIND "${THREE_MF_ASSEMBLY}" "[1,0,0,30" THREE_MF_TRANSLATION)
string(REGEX MATCHALL "\"definition\": \"0:1:1:1\"" THREE_MF_REFS
       "${THREE_MF_ASSEMBLY}")
list(LENGTH THREE_MF_REFS THREE_MF_REF_COUNT)
if(THREE_MF_TRANSLATION EQUAL -1 OR THREE_MF_REF_COUNT LESS 2)
  message(FATAL_ERROR "3MF repeated component or translated instance was lost")
endif()

# Operators can override a misleading or missing extension without renaming data.
file(COPY_FILE "${OUTPUT_DIR}/model.step" "${OUTPUT_DIR}/model.cad")
execute_process(
  COMMAND "${CONVERTER}" convert --format step --input "${OUTPUT_DIR}/model.cad"
          --output "${OUTPUT_DIR}/out-override"
  RESULT_VARIABLE RESULT
  ERROR_VARIABLE ERROR
)
if(NOT RESULT EQUAL 0)
  message(FATAL_ERROR "explicit format override failed: ${ERROR}")
endif()
