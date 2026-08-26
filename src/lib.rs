#![no_std]

use core::{cell::RefCell, ptr};

use critical_section::Mutex;
use embassy_time_queue_utils::Queue;
use embedded_io::Write;
use portable_atomic::{AtomicPtr, AtomicU32, AtomicU64};
use static_cell::StaticCell;

use crate::{
    chanfs::{File, FileMode},
    free_rtos::{
        alloc::FreeRtosAllocator,
        bindings::{RtosTask, RtosTaskParams, xTaskGetCurrentTaskHandle},
        executor::FreeRtosTaskExecutor,
        task::Task,
        time_driver::FreeRtosTimeDriver,
    },
    lwip::{
        bindings::lwip_pcb,
        udp::{UdpChannel, UdpProtocolControlBlock},
    },
    tasks::heartbeat,
};

extern crate alloc;

mod chanfs;
mod free_rtos;
mod log;
mod lwip;
mod panic;
mod tasks;

#[global_allocator]
static ALLOCATOR: FreeRtosAllocator = FreeRtosAllocator;

// Instantiate our Embassy Time Driver the interacts with FreeRTOS.
// Designed for Embassy executors running inside a FreeRTOS task.
embassy_time_driver::time_driver_impl!(static DRIVER: FreeRtosTimeDriver = FreeRtosTimeDriver {
    queue: Mutex::new(RefCell::new(Queue::new())),
    timekeeper: AtomicU64::new(u64::MIN),
    free_rtos_now: AtomicU32::new(u32::MIN),

});

/// Keep track of our FreeRTOS task that is running our Embassy executor.
static TASK: AtomicPtr<RtosTask> = AtomicPtr::new(ptr::null_mut());

/// Static store for our Embassy Executor.
static EXECUTOR: StaticCell<FreeRtosTaskExecutor> = StaticCell::new();

// Static handles for our UDP Service.
static UDP_SERVICE: AtomicPtr<lwip_pcb> = AtomicPtr::new(ptr::null_mut());
static UDP_CHANNEL: StaticCell<UdpChannel> = StaticCell::new();

/// # Safety
/// We will ensure that we call this function in an
/// appropriate place in the Buddy firmware.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn derusting_main() {
    log_info!("derusting_main()");
    match Task::try_from(&TASK) {
        Ok(_) => log_info!("Embassy has been created"),
        // No task (i.e., null ptr) so create it
        Err(_) => match Task::new(c"Embassy", 2048, embassy) {
            Ok(t) => {
                log_info!("Embassy task created.");
                TASK.store(t.as_mut_ptr(), core::sync::atomic::Ordering::SeqCst);
            }
            Err(_) => log_error!("Failed to create Embassy task."),
        },
    }
}

/// Our FreeRTOS Embassy Task that spawns and never returns.
#[unsafe(no_mangle)]
unsafe extern "C" fn embassy(_pv_parameters: *mut RtosTaskParams) -> ! {
    log_info!("Rust Embassy Task");

    let current_task = unsafe { xTaskGetCurrentTaskHandle() };
    if current_task.is_null() {
        log_error!("We should only be called within a FreeRTOS task.");
    }

    // A little FS test.
    if let Some(mut file) = File::open(c"test.txt", FileMode::WRITE | FileMode::CREATE_ALWAYS) {
        log_info!("Test File Opened");
        if let Err(e) = file.write(b"Hello World\n") {
            log_error!("File Error: {e}");
        } else {
            log_info!("File Write Complete");
        };
    }

    // Setting up the UDP service
    let mut udp_channel: Option<&'static UdpChannel> = None;
    lwip::core::with_lwip_core(|core| {
        // UDP Service
        if let Ok(service) = UdpProtocolControlBlock::try_from(&UDP_SERVICE) {
            log_info!("Removing existing UDP service");
            service.remove(&core);
        }
        if let Ok(pcb) = UdpProtocolControlBlock::new(&core) {
            match pcb.bind(9000, &core) {
                Err(_) => {
                    log_error!("Failed to bind on 9000");
                    pcb.remove(&core);
                }
                Ok(_) => {
                    let channel = UDP_CHANNEL.init(UdpChannel::default());
                    pcb.recv(channel, &core);
                    log_info!("UDP Service Available on 9000...");
                    UDP_SERVICE.store(pcb.as_mut_ptr(), core::sync::atomic::Ordering::SeqCst);
                    udp_channel = Some(channel);
                }
            }
        } else {
            log_error!("UDP block not created")
        }
    });

    log_info!("Initialising Executor");
    let executor = EXECUTOR.init(FreeRtosTaskExecutor::new(current_task as _));

    executor.run(|spawner| match heartbeat() {
        Ok(t) => spawner.spawn(t),
        Err(e) => log_error!("Spawn Error: {e}"),
    })
}
