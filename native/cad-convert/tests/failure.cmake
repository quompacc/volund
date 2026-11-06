if(NOT DEFINED GENERATOR OR NOT DEFINED CONVERTER OR NOT DEFINED OUTPUT_DIR)
  message(FATAL_ERROR "GENERATOR, CONVERTER and OUTPUT_DIR are required")
endif()

file(REMOVE_RECURSE "${OUTPUT_DIR}")
file(MAKE_DIRECTORY "${OUTPUT_DIR}")
set(BROKEN_STEP "${OUTPUT_DIR}/broken.step")
set(ARTIFACTS "${OUTPUT_DIR}/artifacts")
file(WRITE "${BROKEN_STEP}" "this is deliberately not a STEP file\n")

execute_process(
  COMMAND "${CONVERTER}" convert --input "${BROKEN_STEP}" --output "${ARTIFACTS}"
  RESULT_VARIABLE CONVERTER_RESULT
  ERROR_VARIABLE CONVERTER_ERROR
)
if(CONVERTER_RESULT EQUAL 0)
  message(FATAL_ERROR "converter unexpectedly accepted an invalid STEP file")
endif()
if(NOT EXISTS "${ARTIFACTS}/diagnostics.json")
  message(FATAL_ERROR "failed conversion produced no diagnostics.json")
endif()
if(EXISTS "${ARTIFACTS}/preview.glb")
  message(FATAL_ERROR "failed conversion left a preview.glb behind")
endif()
if(EXISTS "${ARTIFACTS}/thumbnail.png")
  message(FATAL_ERROR "failed conversion left a thumbnail.png behind")
endif()

file(READ "${ARTIFACTS}/diagnostics.json" DIAGNOSTICS_JSON)
string(JSON STATUS GET "${DIAGNOSTICS_JSON}" status)
string(JSON SEVERITY GET "${DIAGNOSTICS_JSON}" diagnostics 0 severity)
if(NOT STATUS STREQUAL "failed" OR NOT SEVERITY STREQUAL "error")
  message(FATAL_ERROR "failure diagnostics have the wrong status or severity")
endif()

set(EMPTY_STEP "${OUTPUT_DIR}/empty.step")
set(EMPTY_ARTIFACTS "${OUTPUT_DIR}/empty-artifacts")
execute_process(
  COMMAND "${GENERATOR}" "${EMPTY_STEP}" --empty
  RESULT_VARIABLE GENERATOR_RESULT
  ERROR_VARIABLE GENERATOR_ERROR
)
if(NOT GENERATOR_RESULT EQUAL 0)
  message(FATAL_ERROR "empty STEP fixture generation failed: ${GENERATOR_ERROR}")
endif()
execute_process(
  COMMAND "${CONVERTER}" convert --input "${EMPTY_STEP}" --output "${EMPTY_ARTIFACTS}"
  RESULT_VARIABLE EMPTY_RESULT
)
if(EMPTY_RESULT EQUAL 0 OR EXISTS "${EMPTY_ARTIFACTS}/preview.glb")
  message(FATAL_ERROR "converter accepted an empty STEP geometry")
endif()
if(NOT EXISTS "${EMPTY_ARTIFACTS}/diagnostics.json")
  message(FATAL_ERROR "empty STEP geometry produced no diagnostics")
endif()
file(READ "${EMPTY_ARTIFACTS}/diagnostics.json" EMPTY_DIAGNOSTICS)
if(NOT EMPTY_DIAGNOSTICS MATCHES "meshing produced zero triangles")
  message(FATAL_ERROR "empty STEP geometry produced no actionable diagnostic")
endif()

# Some CAD exports carry only product metadata and no shape representation.
set(METADATA_STEP "${OUTPUT_DIR}/metadata-only.step")
set(METADATA_ARTIFACTS "${OUTPUT_DIR}/metadata-only-artifacts")
file(WRITE "${METADATA_STEP}" "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('metadata only'),'2;1');
FILE_NAME('metadata-only.step','2026-11-06T00:00:00',(''),(''),'test','test','');
FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));
ENDSEC;
DATA;
#1=APPLICATION_CONTEXT('configuration controlled 3d designs of mechanical parts and assemblies');
#2=MECHANICAL_CONTEXT('NONE',#1,'mechanical');
#3=PRODUCT('part','part','',(#2));
#4=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('1','',#3,.NOT_KNOWN.);
#5=DESIGN_CONTEXT('detailed design',#1,'design');
#6=PRODUCT_DEFINITION('design','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('NONE','NONE',#6);
ENDSEC;
END-ISO-10303-21;
")
execute_process(
  COMMAND "${CONVERTER}" convert --input "${METADATA_STEP}" --output "${METADATA_ARTIFACTS}"
  RESULT_VARIABLE METADATA_RESULT
)
if(METADATA_RESULT EQUAL 0 OR EXISTS "${METADATA_ARTIFACTS}/preview.glb")
  message(FATAL_ERROR "converter accepted a STEP file without geometry")
endif()
file(READ "${METADATA_ARTIFACTS}/diagnostics.json" METADATA_DIAGNOSTICS)
if(NOT METADATA_DIAGNOSTICS MATCHES "STEP file contains no geometry")
  message(FATAL_ERROR "metadata-only STEP produced no actionable diagnostic: ${METADATA_DIAGNOSTICS}")
endif()

set(UNSUPPORTED_SOURCE "${OUTPUT_DIR}/unsupported.xyz")
set(UNSUPPORTED_ARTIFACTS "${OUTPUT_DIR}/unsupported-artifacts")
file(WRITE "${UNSUPPORTED_SOURCE}" "unsupported fixture\n")
execute_process(
  COMMAND "${CONVERTER}" convert --input "${UNSUPPORTED_SOURCE}"
          --output "${UNSUPPORTED_ARTIFACTS}"
  RESULT_VARIABLE UNSUPPORTED_RESULT
  ERROR_VARIABLE UNSUPPORTED_ERROR
)
if(UNSUPPORTED_RESULT EQUAL 0 OR
   NOT UNSUPPORTED_ERROR MATCHES "unsupported CAD format: xyz")
  message(FATAL_ERROR "converter did not clearly reject an unsupported format")
endif()
