#include "libderusting.hpp"

// src/common/marlin_server.cpp
#include "marlin_server.hpp"
#include "logging/log.hpp"
#include "lwip/tcp.h"
#include "lwip/tcpip.h"
#include <semphr.h>
#include <task.h>

LOG_COMPONENT_DEF(derusting, logging::Severity::info);

// Forwards a log message from Rust to the firmware's own logger, under the
// `derusting` log component.
extern "C" void derusting_log_event(logging::Severity severity, const char* msg) {
  log_event(severity, derusting, "%s", msg);
}

// Submits a line of gcode to Marlin's command queue from Rust.
extern "C" bool derusting_gcode_cmd(const char* cmd) {
  log_info(derusting, "Gcode from Rust: %s", cmd);
  return marlin_server::enqueue_gcode_try(cmd);
}


// Reports whether Marlin's print engine is currently idle.
extern "C" bool derusting_is_idle() {
  return marlin_server::printer_idle();
}


// Wraps lwIP's tcp_sndbuf() so Rust can query a pcb's free send buffer.
extern "C" u16_t derusting_tcp_sndbuf(const struct tcp_pcb *pcb) {
    return tcp_sndbuf(pcb);
}

// Reports whether the calling task already holds lwIP's tcpip core lock,
// so Rust knows whether it needs to take it before calling into lwIP.
extern "C" bool derusting_holds_tcpip_core_lock(void) {
  #if LWIP_TCPIP_CORE_LOCKING
      SemaphoreHandle_t core_lock = reinterpret_cast<SemaphoreHandle_t>(lock_tcpip_core);
      return xSemaphoreGetMutexHolder(core_lock) == xTaskGetCurrentTaskHandle();
  #else
    return false;
  #endif
}

// libderusting.cpp
// Reports whether lwIP is holding data on this pcb that the receive
// callback previously refused (returned non-OK for).
extern "C" bool derusting_tcp_has_refused_data(const struct tcp_pcb *pcb) {
  return pcb->refused_data != nullptr;
}

