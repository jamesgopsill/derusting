#include "libderusting.hpp"

// src/common/marlin_server.cpp
#include "marlin_server.hpp"
#include "logging/log.hpp"
#include "lwip/tcp.h"
#include "lwip/tcpip.h"
#include <semphr.h>
#include <task.h>

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

extern "C" bool derusting_holds_tcpip_core_lock(void) {
  #if LWIP_TCPIP_CORE_LOCKING
      SemaphoreHandle_t core_lock = reinterpret_cast<SemaphoreHandle_t>(lock_tcpip_core);
      return xSemaphoreGetMutexHolder(core_lock) == xTaskGetCurrentTaskHandle();
  #else
    return false;
  #endif
}
