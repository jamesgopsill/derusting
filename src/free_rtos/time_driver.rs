use core::{cell::RefCell, sync::atomic::Ordering, task::Waker};

use critical_section::Mutex;
use embassy_time_driver::Driver;
use embassy_time_queue_utils::Queue;
use portable_atomic::{AtomicU32, AtomicU64};

use crate::free_rtos::bindings::*;

/// An Embassy Time Driver that can provide time-based software interrupts
/// within a FreeRTOS task. `embassy-time` is set to `tick-hz-1_000`
pub struct FreeRtosTimeDriver {
    pub timekeeper: AtomicU64,
    pub free_rtos_now: AtomicU32,
    pub queue: Mutex<RefCell<Queue>>,
}

impl Driver for FreeRtosTimeDriver {
    // Calculate now. FreeRTOS runs a u32 timer that could wrap after 49 days
    // which could happen with a 3D printer. Embassy Time Driver works on a
    // u64 so we keep track of the free_rts time, `wrap_sub` and add to our
    // own timekeeper.
    fn now(&self) -> u64 {
        critical_section::with(|_cs| {
            let free_rtos_now = unsafe { xTaskGetTickCount() };
            let previous_free_rtos_now = self.free_rtos_now.load(Ordering::SeqCst);
            let tick_diff = free_rtos_now.wrapping_sub(previous_free_rtos_now);
            if tick_diff > 0 {
                self.free_rtos_now.store(free_rtos_now, Ordering::SeqCst);
                self.timekeeper.add(tick_diff as u64, Ordering::SeqCst);
            }
            self.timekeeper.load(Ordering::SeqCst)
        })
    }

    /// Embassy has informed us of a new time to wake. Update our
    /// wake up time.
    fn schedule_wake(&self, at: u64, waker: &Waker) {
        critical_section::with(|cs| {
            let mut queue = self.queue.borrow(cs).borrow_mut();
            queue.schedule_wake(at, waker);
        })
    }
}

impl FreeRtosTimeDriver {
    pub fn next_expiration(&self) -> u64 {
        let now = self.now();
        critical_section::with(|cs| self.queue.borrow(cs).borrow_mut().next_expiration(now))
    }

    pub fn wait_for_interrupt_or_timeout(&self) {
        let now_ticks = self.now();
        let exp_ticks = self.next_expiration();
        let diff_ticks = exp_ticks.saturating_sub(now_ticks);
        unsafe { ulTaskGenericNotifyTake(0, 1, diff_ticks as u32) };
        let _ = self.next_expiration();
    }
}
