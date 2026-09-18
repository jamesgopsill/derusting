#include "libderusting.hpp"

// src/common/marlin_server.cpp
#include "marlin_server.hpp"
#include "logging/log.hpp"
#include "lwip/tcp.h"

LOG_COMPONENT_DEF(derusting, logging::Severity::info);

extern "C" void derusting_log_event(logging::Severity severity, const char* msg) {
  log_event(severity, derusting, "%s", msg);
}

extern "C" bool derusting_gcode_cmd(const char* cmd) {
  log_info(derusting, "Gcode from Rust: %s", cmd);
  return marlin_server::enqueue_gcode_try(cmd);
}


extern "C" bool derusting_is_idle() {
  return marlin_server::printer_idle();
}


extern "C" u16_t derusting_tcp_sndbuf(const struct tcp_pcb *pcb) {
    return tcp_sndbuf(pcb);
}
