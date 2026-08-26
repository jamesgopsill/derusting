#include "marlin_client.hpp"
#include "logging/log.hpp"

LOG_COMPONENT_DEF(derusting, logging::Severity::info);

extern "C" void derusting_log_event(logging::Severity severity, const char* msg) {
  log_event(severity, derusting, "%s", msg);
}

extern "C" void derusting_gcode_cmd(const char* cmd) {
  marlin_client::gcode(cmd);
}


