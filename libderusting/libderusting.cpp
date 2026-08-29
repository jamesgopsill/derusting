#include "libderusting.hpp"

#include "marlin_server.hpp"
#include "marlin_client.hpp"
#include "logging/log.hpp"

LOG_COMPONENT_DEF(derusting, logging::Severity::info);

extern "C" void derusting_log_event(logging::Severity severity, const char* msg) {
  log_event(severity, derusting, "%s", msg);
}

extern "C" void derusting_gcode_cmd(const char* cmd) {
  log_info(derusting, "Gcode from Rust: %s", cmd);
  if (marlin_server::is_marlin_server_thread()) {
    log_info(derusting, "Marlin Server Thread");
    marlin_server::enqueue_gcode_printf("%s", cmd);
  } else {
    log_info(derusting, "Not Marlin Server Thread");
    marlin_client::gcode_printf("%s", cmd);
  }
  // marlin_client::gcode(cmd)
  // marlin_client::gcode_printf("%s", cmd);
}


