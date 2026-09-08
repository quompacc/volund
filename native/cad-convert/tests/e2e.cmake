if(NOT DEFINED GENERATOR OR NOT DEFINED CONVERTER OR NOT DEFINED OUTPUT_DIR)
  message(FATAL_ERROR "GENERATOR, CONVERTER and OUTPUT_DIR are required")
endif()

file(REMOVE_RECURSE "${OUTPUT_DIR}")
file(MAKE_DIRECTORY "${OUTPUT_DIR}")
set(FIXTURE "${OUTPUT_DIR}/fixture.step")
set(ARTIFACTS "${OUTPUT_DIR}/artifacts")
set(REPEATED_ARTIFACTS "${OUTPUT_DIR}/artifacts-repeat")

execute_process(
  COMMAND "${GENERATOR}" "${FIXTURE}"
  RESULT_VARIABLE GENERATOR_RESULT
  ERROR_VARIABLE GENERATOR_ERROR
)
if(NOT GENERATOR_RESULT EQUAL 0)
  message(FATAL_ERROR "fixture generator failed: ${GENERATOR_ERROR}")
endif()

execute_process(
  COMMAND "${CONVERTER}" convert --input "${FIXTURE}" --output "${ARTIFACTS}"
  RESULT_VARIABLE CONVERTER_RESULT
  ERROR_VARIABLE CONVERTER_ERROR
)
if(NOT CONVERTER_RESULT EQUAL 0)
  message(FATAL_ERROR "converter failed: ${CONVERTER_ERROR}")
endif()

foreach(ARTIFACT preview.glb thumbnail.png assembly.json diagnostics.json result.json)
  if(NOT EXISTS "${ARTIFACTS}/${ARTIFACT}")
    message(FATAL_ERROR "missing artifact: ${ARTIFACT}")
  endif()
endforeach()
file(READ "${ARTIFACTS}/thumbnail.png" PNG_MAGIC OFFSET 0 LIMIT 8 HEX)
if(NOT PNG_MAGIC STREQUAL "89504e470d0a1a0a")
  message(FATAL_ERROR "thumbnail.png does not have the PNG signature")
endif()
execute_process(
  COMMAND "${CONVERTER}" convert --input "${FIXTURE}" --output "${REPEATED_ARTIFACTS}"
  RESULT_VARIABLE REPEATED_RESULT
  ERROR_VARIABLE REPEATED_ERROR
)
if(NOT REPEATED_RESULT EQUAL 0)
  message(FATAL_ERROR "repeated converter run failed: ${REPEATED_ERROR}")
endif()
file(SHA256 "${ARTIFACTS}/thumbnail.png" THUMBNAIL_HASH)
file(SHA256 "${REPEATED_ARTIFACTS}/thumbnail.png" REPEATED_THUMBNAIL_HASH)
if(NOT THUMBNAIL_HASH STREQUAL REPEATED_THUMBNAIL_HASH)
  message(FATAL_ERROR "thumbnail generation is not deterministic")
endif()

file(SIZE "${ARTIFACTS}/preview.glb" GLB_SIZE)
if(GLB_SIZE LESS 100)
  message(FATAL_ERROR "preview.glb is unexpectedly small: ${GLB_SIZE} bytes")
endif()
file(READ "${ARTIFACTS}/preview.glb" GLB_MAGIC OFFSET 0 LIMIT 4 HEX)
if(NOT GLB_MAGIC STREQUAL "676c5446")
  message(FATAL_ERROR "preview does not have the binary glTF magic header")
endif()

if(ASSIMP AND EXISTS "${ASSIMP}")
  execute_process(
    COMMAND "${ASSIMP}" info "${ARTIFACTS}/preview.glb"
    RESULT_VARIABLE ASSIMP_RESULT
    OUTPUT_VARIABLE ASSIMP_INFO
    ERROR_VARIABLE ASSIMP_ERROR
  )
  if(NOT ASSIMP_RESULT EQUAL 0)
    message(FATAL_ERROR "cannot inspect preview orientation: ${ASSIMP_ERROR}")
  endif()
  string(FIND "${ASSIMP_INFO}" "Minimum point      (0.000000 0.000000 -0.010000)" MINIMUM_FOUND)
  string(FIND "${ASSIMP_INFO}" "Maximum point      (0.050000 0.021000 0.000000)" MAXIMUM_FOUND)
  if(MINIMUM_FOUND EQUAL -1 OR MAXIMUM_FOUND EQUAL -1)
    message(FATAL_ERROR "GLB is not converted from CAD Z-up to glTF Y-up:\n${ASSIMP_INFO}")
  endif()
endif()

file(SHA256 "${FIXTURE}" EXPECTED_HASH)
file(READ "${ARTIFACTS}/result.json" RESULT_JSON)
string(JSON STATUS GET "${RESULT_JSON}" status)
string(JSON ACTUAL_HASH GET "${RESULT_JSON}" source sha256)
string(JSON DEFINITION_COUNT GET "${RESULT_JSON}" assembly definitionCount)
string(JSON INSTANCE_COUNT GET "${RESULT_JSON}" assembly instanceCount)
string(JSON TRIANGLE_COUNT GET "${RESULT_JSON}" preview triangleCount)
string(JSON PROFILE GET "${RESULT_JSON}" preview profile)
string(JSON MODEL_DIAGONAL GET "${RESULT_JSON}" preview modelDiagonal)
string(JSON LINEAR_DEFLECTION GET "${RESULT_JSON}" preview linearDeflection)
if(NOT STATUS STREQUAL "ready")
  message(FATAL_ERROR "unexpected conversion status: ${STATUS}")
endif()
if(NOT ACTUAL_HASH STREQUAL EXPECTED_HASH)
  message(FATAL_ERROR "SHA-256 mismatch: ${ACTUAL_HASH} != ${EXPECTED_HASH}")
endif()
if(NOT DEFINITION_COUNT EQUAL 3)
  message(FATAL_ERROR "expected 3 definitions, got ${DEFINITION_COUNT}")
endif()
if(NOT INSTANCE_COUNT EQUAL 3)
  message(FATAL_ERROR "expected 3 instances, got ${INSTANCE_COUNT}")
endif()
if(TRIANGLE_COUNT LESS 1)
  message(FATAL_ERROR "expected a non-empty mesh")
endif()
if(NOT PROFILE STREQUAL "web")
  message(FATAL_ERROR "expected the default web profile, got ${PROFILE}")
endif()
if(MODEL_DIAGONAL LESS_EQUAL 0 OR LINEAR_DEFLECTION LESS_EQUAL 0)
  message(FATAL_ERROR "adaptive mesh settings must be positive")
endif()

file(READ "${ARTIFACTS}/assembly.json" ASSEMBLY_JSON)
string(REGEX MATCHALL "\"id\": \"node-[0-9]+\"" NODE_IDS "${ASSEMBLY_JSON}")
list(LENGTH NODE_IDS NODE_ID_COUNT)
list(REMOVE_DUPLICATES NODE_IDS)
list(LENGTH NODE_IDS UNIQUE_NODE_ID_COUNT)
if(NOT NODE_ID_COUNT EQUAL UNIQUE_NODE_ID_COUNT)
  message(FATAL_ERROR "assembly node identifiers must be globally unique")
endif()

string(JSON DEFINITION_COUNT LENGTH "${ASSEMBLY_JSON}" definitions)
set(DEFINITION_IDS)
if(DEFINITION_COUNT GREATER 0)
  math(EXPR LAST_DEFINITION "${DEFINITION_COUNT} - 1")
  foreach(INDEX RANGE 0 ${LAST_DEFINITION})
    string(JSON DEFINITION_ID GET "${ASSEMBLY_JSON}" definitions ${INDEX} id)
    list(APPEND DEFINITION_IDS "${DEFINITION_ID}")
  endforeach()
endif()
function(validate_assembly_node NODE_JSON)
  string(JSON NODE_DEFINITION GET "${NODE_JSON}" definition)
  list(FIND DEFINITION_IDS "${NODE_DEFINITION}" DEFINITION_INDEX)
  if(DEFINITION_INDEX EQUAL -1)
    message(FATAL_ERROR "assembly node references missing definition: ${NODE_DEFINITION}")
  endif()
  string(JSON CHILD_COUNT LENGTH "${NODE_JSON}" children)
  if(CHILD_COUNT GREATER 0)
    math(EXPR LAST_CHILD "${CHILD_COUNT} - 1")
    foreach(CHILD_INDEX RANGE 0 ${LAST_CHILD})
      string(JSON CHILD_JSON GET "${NODE_JSON}" children ${CHILD_INDEX})
      validate_assembly_node("${CHILD_JSON}")
    endforeach()
  endif()
endfunction()
string(JSON ROOT_COUNT LENGTH "${ASSEMBLY_JSON}" roots)
if(ROOT_COUNT GREATER 0)
  math(EXPR LAST_ROOT "${ROOT_COUNT} - 1")
  foreach(ROOT_INDEX RANGE 0 ${LAST_ROOT})
    string(JSON ROOT_JSON GET "${ASSEMBLY_JSON}" roots ${ROOT_INDEX})
    validate_assembly_node("${ROOT_JSON}")
  endforeach()
endif()
string(JSON COLOR_SPACE GET "${ASSEMBLY_JSON}" colorSpace)
if(NOT COLOR_SPACE STREQUAL "sRGB")
  message(FATAL_ERROR "unexpected color space: ${COLOR_SPACE}")
endif()
string(JSON BLOCK_RED GET "${ASSEMBLY_JSON}" definitions 1 color 0)
string(JSON BLOCK_GREEN GET "${ASSEMBLY_JSON}" definitions 1 color 1)
string(JSON BLOCK_BLUE GET "${ASSEMBLY_JSON}" definitions 1 color 2)
string(JSON AXLE_RED GET "${ASSEMBLY_JSON}" definitions 2 color 0)
string(JSON AXLE_GREEN GET "${ASSEMBLY_JSON}" definitions 2 color 1)
string(JSON AXLE_BLUE GET "${ASSEMBLY_JSON}" definitions 2 color 2)
if(BLOCK_RED LESS 0.80 OR BLOCK_RED GREATER 0.84 OR
   BLOCK_GREEN LESS 0.14 OR BLOCK_GREEN GREATER 0.18 OR
   BLOCK_BLUE LESS 0.06 OR BLOCK_BLUE GREATER 0.10)
  message(FATAL_ERROR "assembly manifest lost the forge-block sRGB color")
endif()
if(AXLE_RED LESS 0.06 OR AXLE_RED GREATER 0.10 OR
   AXLE_GREEN LESS 0.23 OR AXLE_GREEN GREATER 0.27 OR
   AXLE_BLUE LESS 0.80 OR AXLE_BLUE GREATER 0.84)
  message(FATAL_ERROR "assembly manifest lost the axle sRGB color")
endif()
foreach(
  EXPECTED_NAME
  "VÖLUND Test Assembly"
  "Forge Block"
  "Axle"
  "Base Instance"
  "Base Copy"
  "Axle Instance"
)
  string(FIND "${ASSEMBLY_JSON}" "${EXPECTED_NAME}" FOUND_AT)
  if(FOUND_AT EQUAL -1)
    message(FATAL_ERROR "assembly manifest lost name: ${EXPECTED_NAME}")
  endif()
endforeach()
string(FIND "${ASSEMBLY_JSON}" "[1,0,0,30,0,1,0,0" TRANSLATION_FOUND_AT)
if(TRANSLATION_FOUND_AT EQUAL -1)
  message(FATAL_ERROR "assembly manifest lost the translated block transform")
endif()
string(FIND "${ASSEMBLY_JSON}" "\"properties\": {\"labelEntry\"" PROPERTIES_FOUND_AT)
if(PROPERTIES_FOUND_AT EQUAL -1)
  message(FATAL_ERROR "assembly manifest lost bounded definition properties")
endif()
