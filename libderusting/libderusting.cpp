#include "libderusting.hpp"

// src/common/marlin_server.cpp
#include "marlin_server.hpp"
#include "logging/log.hpp"
#include "lwip/netif.h"
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


// Returns the IPv4 address in network byte order, or 0 if the link is down.
extern "C" uint32_t derusting_local_ipv4(void) {
    uint32_t addr = 0;
    LOCK_TCPIP_CORE();
    if (netif_default && netif_is_up(netif_default) && netif_is_link_up(netif_default)) {
        addr = ip4_addr_get_u32(netif_ip4_addr(netif_default));
    }
    UNLOCK_TCPIP_CORE();
    return addr;
}
