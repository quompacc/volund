if(NOT DEFINED REPO_ROOT)
  message(FATAL_ERROR "REPO_ROOT is required")
endif()

file(GLOB_RECURSE SOURCE_FILES LIST_DIRECTORIES FALSE
  "${REPO_ROOT}/apps/*.rs"
  "${REPO_ROOT}/crates/*.rs"
  "${REPO_ROOT}/native/*.c"
  "${REPO_ROOT}/native/*.cc"
  "${REPO_ROOT}/native/*.cpp"
  "${REPO_ROOT}/native/*.h"
  "${REPO_ROOT}/native/*.hpp"
  "${REPO_ROOT}/native/*.hxx"
  "${REPO_ROOT}/apps/*.ts"
  "${REPO_ROOT}/apps/*.tsx"
  "${REPO_ROOT}/apps/*.sql"
  "${REPO_ROOT}/migrations/*.sql"
)
list(FILTER SOURCE_FILES EXCLUDE REGEX
  "/(build|target|vendor|third_party|node_modules)/")

foreach(SOURCE_FILE IN LISTS SOURCE_FILES)
  file(READ "${SOURCE_FILE}" CONTENTS)
  string(REGEX REPLACE "[^\n]" "" NEWLINES "${CONTENTS}")
  string(LENGTH "${NEWLINES}" LINE_COUNT)
  string(LENGTH "${CONTENTS}" CONTENT_LENGTH)
  if(CONTENT_LENGTH GREATER 0)
    math(EXPR LAST_INDEX "${CONTENT_LENGTH} - 1")
    string(SUBSTRING "${CONTENTS}" ${LAST_INDEX} 1 LAST_CHARACTER)
    if(NOT LAST_CHARACTER STREQUAL "\n")
      math(EXPR LINE_COUNT "${LINE_COUNT} + 1")
    endif()
  endif()
  if(LINE_COUNT GREATER 600)
    file(RELATIVE_PATH RELATIVE_FILE "${REPO_ROOT}" "${SOURCE_FILE}")
    message(FATAL_ERROR
      "${RELATIVE_FILE} has ${LINE_COUNT} lines; handwritten source is limited to 600")
  endif()
endforeach()
