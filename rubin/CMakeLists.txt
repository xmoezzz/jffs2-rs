project(rubin)
cmake_minimum_required(VERSION 3.0)

################################################################################
# Source groups
################################################################################
set(Headers
    "rubin.h"
)
source_group("Headers" FILES ${Headers})

set(Sources
    "rubin_compr.c"
)
source_group("Sources" FILES ${Sources})

set(ALL_FILES
    ${Headers}
    ${Sources}
)

################################################################################
# Target
################################################################################
add_library(${PROJECT_NAME} STATIC ${ALL_FILES})

#use_props(${PROJECT_NAME} "${CMAKE_CONFIGURATION_TYPES}" "${DEFAULT_CXX_PROPS}")
set(ROOT_NAMESPACE rubin)


################################################################################
# Include directories
################################################################################
target_include_directories(${PROJECT_NAME} PUBLIC
    "${CMAKE_CURRENT_SOURCE_DIR}/."
)


if (NOT MSVC)
    target_compile_options(${PROJECT_NAME} PRIVATE "-fPIE")
    target_compile_options(${PROJECT_NAME} PRIVATE "-Wno-implicit-function-declaration")
endif()

################################################################################
# Dependencies
################################################################################
set(ADDITIONAL_LIBRARY_DEPENDENCIES
    "mscoree"
)
target_link_libraries(${PROJECT_NAME} PUBLIC "${ADDITIONAL_LIBRARY_DEPENDENCIES}")

include(GNUInstallDirs)
install(TARGETS ${PROJECT_NAME} ARCHIVE DESTINATION lib COMPONENT library)


