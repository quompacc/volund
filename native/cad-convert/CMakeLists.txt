cmake_minimum_required(VERSION 3.25)
project(volund-cad-convert VERSION 0.42.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

find_package(
  OpenCASCADE REQUIRED
  COMPONENTS
    FoundationClasses
    ModelingData
    ModelingAlgorithms
    Visualization
    ApplicationFramework
    DataExchange
)
find_package(assimp REQUIRED CONFIG)
find_package(ZLIB REQUIRED)

add_executable(
  volund-cad-convert
  src/main.cpp
  src/importer.cpp
  src/options.cpp
  src/png_writer.cpp
  src/sha256.cpp
  src/shaded_raster.cpp
  src/thumbnail.cpp
  src/three_mf.cpp
)
target_include_directories(volund-cad-convert PRIVATE ${OpenCASCADE_INCLUDE_DIR})
target_compile_definitions(
  volund-cad-convert
  PRIVATE VOLUND_CAD_CONVERT_VERSION="${PROJECT_VERSION}"
)

# Do not use OpenCASCADE_LIBRARIES here. Debian's OCCT package includes the
# optional DRAW test module in that umbrella list, while production systems do
# not install its libraries. These imported targets pull in only the transitive
# dependencies required for STEP/XCAF import, meshing, and GLB export.
target_link_libraries(
  volund-cad-convert
  PRIVATE
    TKDESTEP
    TKDEIGES
    TKDEGLTF
    TKDEOBJ
    TKBRep
    TKMesh
    assimp::assimp
    ZLIB::ZLIB
)

if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
  target_compile_options(volund-cad-convert PRIVATE -Wall -Wextra -Wpedantic -Werror)
endif()

install(TARGETS volund-cad-convert RUNTIME DESTINATION bin)

include(CTest)
if(BUILD_TESTING)
  find_program(ASSIMP_EXECUTABLE assimp)
  add_executable(volund-cad-fixture tests/fixture_generator.cpp)
  target_include_directories(volund-cad-fixture PRIVATE ${OpenCASCADE_INCLUDE_DIR})
  target_link_libraries(volund-cad-fixture PRIVATE TKDESTEP TKDEIGES TKBRep TKMesh)
  if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    target_compile_options(volund-cad-fixture PRIVATE -Wall -Wextra -Wpedantic -Werror)
  endif()

  add_executable(volund-thumbnail-test tests/thumbnail_test.cpp src/shaded_raster.cpp src/png_writer.cpp)
  target_link_libraries(volund-thumbnail-test PRIVATE ZLIB::ZLIB)
  if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    target_compile_options(volund-thumbnail-test PRIVATE -Wall -Wextra -Wpedantic -Werror)
  endif()
  add_test(NAME thumbnail-renderer COMMAND volund-thumbnail-test)

  add_executable(volund-three-mf-test tests/three_mf_test.cpp src/three_mf.cpp)
  target_link_libraries(volund-three-mf-test PRIVATE ZLIB::ZLIB)
  if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    target_compile_options(volund-three-mf-test PRIVATE -Wall -Wextra -Wpedantic -Werror)
  endif()
  add_test(NAME three-mf-reader COMMAND volund-three-mf-test ${CMAKE_CURRENT_BINARY_DIR}/three-mf-test)

  add_test(
    NAME cad-convert-e2e
    COMMAND
      ${CMAKE_COMMAND}
      -DGENERATOR=$<TARGET_FILE:volund-cad-fixture>
      -DCONVERTER=$<TARGET_FILE:volund-cad-convert>
      -DASSIMP=${ASSIMP_EXECUTABLE}
      -DOUTPUT_DIR=${CMAKE_CURRENT_BINARY_DIR}/test-output
      -P ${CMAKE_CURRENT_SOURCE_DIR}/tests/e2e.cmake
  )
  add_test(
    NAME cad-convert-invalid-step
    COMMAND
      ${CMAKE_COMMAND}
      -DGENERATOR=$<TARGET_FILE:volund-cad-fixture>
      -DCONVERTER=$<TARGET_FILE:volund-cad-convert>
      -DOUTPUT_DIR=${CMAKE_CURRENT_BINARY_DIR}/test-failure-output
      -P ${CMAKE_CURRENT_SOURCE_DIR}/tests/failure.cmake
  )
  add_test(
    NAME source-file-size
    COMMAND
      ${CMAKE_COMMAND}
      -DREPO_ROOT=${CMAKE_CURRENT_SOURCE_DIR}/../..
      -P ${CMAKE_CURRENT_SOURCE_DIR}/tests/source_size.cmake
  )
  if(ASSIMP_EXECUTABLE)
    add_test(
      NAME cad-convert-format-matrix
      COMMAND
        ${CMAKE_COMMAND}
        -DGENERATOR=$<TARGET_FILE:volund-cad-fixture>
        -DCONVERTER=$<TARGET_FILE:volund-cad-convert>
        -DASSIMP=${ASSIMP_EXECUTABLE}
        -DOUTPUT_DIR=${CMAKE_CURRENT_BINARY_DIR}/format-matrix-output
        -P ${CMAKE_CURRENT_SOURCE_DIR}/tests/format_matrix.cmake
    )
  endif()
endif()
