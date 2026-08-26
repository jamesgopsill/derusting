target_include_directories(firmware PRIVATE .)

target_sources(
  firmware
  PRIVATE libderusting.cpp
)

file(GLOB RUST_LIBS "./*.a")
target_link_libraries(firmware PRIVATE ${RUST_LIBS})
target_link_options(firmware PRIVATE
  "-Wl,--start-group"
  ${RUST_LIBS}
  "-Wl,--end-group"
)
