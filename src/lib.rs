#![no_std]

use core::{cell::RefCell, ptr, sync::atomic::Ordering};

use alloc::vec::Vec;
use critical_section::Mutex;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time_queue_utils::Queue;
use embedded_io::Write;
use picoserve::{Config, Router, Server, routing::get};
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
        pico::PicoSocket,
        tcp::{TcpProtocolControlBlock, on_tcp_accept},
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

// Static handles for our TCP Service.
static TCP_SERVICE: AtomicPtr<lwip_pcb> = AtomicPtr::new(ptr::null_mut());
static TCP_CHANNELS: StaticCell<
    Channel<CriticalSectionRawMutex, PicoSocket<picoserve::EmbassyRuntime>, 1>,
> = StaticCell::new();

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
    let tcp_channels = TCP_CHANNELS.init(Channel::new());
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

        // TCP Service
        if let Ok(tcp) = TcpProtocolControlBlock::try_from(&TCP_SERVICE) {
            log_info!("Removing existing TCP service");
            let _ = tcp.close_with_core(&core);
        }

        if let Ok(tcp) = TcpProtocolControlBlock::new(&core) {
            let err = tcp.bind(8080);
            match err {
                Ok(_) => {
                    if let Ok(tcp) = tcp.listen_with_backlog(2, &core) {
                        tcp.accept(Some(on_tcp_accept), &core);
                        TCP_SERVICE.store(tcp.as_mut_ptr(), Ordering::SeqCst);
                        log_info!("TCP UP on 8080...");
                    }
                }
                Err(_) => {
                    let _ = tcp.close_with_core(&core);
                }
            }
        } else {
            log_error!("TCP block not created");
        }
    });

    log_info!("Initialising Executor");
    let executor = EXECUTOR.init(FreeRtosTaskExecutor::new(current_task as _));

    executor.run(|spawner| {
        match heartbeat() {
            Ok(t) => spawner.spawn(t),
            Err(e) => log_error!("Spawn Error: {e}"),
        }
        match tcp_channels_handler(spawner, tcp_channels) {
            Ok(t) => spawner.spawn(t),
            Err(e) => log_error!("Spawn Error: {e}"),
        }
    })
}

#[embassy_executor::task(pool_size = 1)]
pub async fn tcp_channels_handler(
    spawner: Spawner,
    tcp_channels: &'static Channel<
        CriticalSectionRawMutex,
        PicoSocket<picoserve::EmbassyRuntime>,
        1,
    >,
) {
    loop {
        let channel = tcp_channels.receive().await;
        match tcp_channel_handler(channel) {
            Ok(t) => spawner.spawn(t),
            Err(e) => log_error!("Spawn Error: {e}"),
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
pub async fn tcp_channel_handler(ch: PicoSocket<picoserve::EmbassyRuntime>) {
    let app = Router::new().route("/", get(|| async { "Hello World" }));
    let config = Config::default();
    // let mut http_buffer = [0u8; 512];
    let mut http_buffer: Vec<u8> = Vec::with_capacity(2048);
    let server = Server::new(&app, &config, &mut http_buffer);
    let _ = server.serve(ch).await;
}

/*

#[embassy_executor::task(pool_size = 1)]
pub async fn tcp_channels_handler(
    spawner: Spawner,
    tcp_channels: &'static Channel<CriticalSectionRawMutex, TcpChannel, 1>,
) {
    loop {
        let channel = tcp_channels.receive().await;
        match tcp_channel_handler(channel) {
            Ok(t) => spawner.spawn(t),
            Err(e) => log_error!("Spawn Error: {e}"),
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
pub async fn tcp_channel_handler(ch: TcpChannel) {
    // TODO: would loop in reality.
    let _bytes = ch.receive().await;
    log_info!("Bytes Received");
    let _ = ch.write(OK.as_bytes());
}

pub const OK: &str = "HTTP/1.1 200 OK\r\nContent-length:4\r\nConnection: close\r\n\r\npong";
*/
