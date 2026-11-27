#![no_std]
#![no_main]

use cube_syscall as sys;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{DeviceCapabilities, Medium};
use smoltcp::time::Instant;
use cube_syscall::net::{NetRingBuffer, RING_SIZE, PACKET_DATA_SIZE};

const SHARED_ADDR: u64 = 0x5000_0000;

// Заглушка для PHY, пока не настроим Shared Memory с драйвером
struct CubePhy;

impl smoltcp::phy::Device for CubePhy {
    type RxToken<'a> = CubeRx;
    type TxToken<'a> = CubeTx;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        // Тут будем читать из Shared Memory драйвера net_e1000
        None
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(CubeTx)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1500;
        caps
    }
}

struct CubeRx;
impl smoltcp::phy::RxToken for CubeRx {
    fn consume<R, F>(self, _f: F) -> R
    where F: FnOnce(&[u8]) -> R {
        unreachable!()
    }
}

struct CubeTx;
impl smoltcp::phy::TxToken for CubeTx {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where F: FnOnce(&mut [u8]) -> R {
        let mut buf = vec![0u8; len]; // В no_std нужен alloc
        let result = f(&mut buf);
        // Тут отправляем buf в Shared Memory драйвера
        result
    }
}

fn poll_driver() {
    unsafe {
        let buf = &*(SHARED_ADDR as *const NetRingBuffer);

        let current_tail = buf.tail.load(Ordering::Relaxed);
        let current_head = buf.head.load(Ordering::Acquire);

        if current_tail == current_head {
            return; // Нет новых пакетов
        }

        let desc_idx = current_tail as usize;
        let desc = &buf.descriptors[desc_idx];

        let len = core::ptr::read_volatile(&desc.len) as usize;
        let offset = core::ptr::read_volatile(&desc.offset) as usize;

        if len > 0 && len <= PACKET_DATA_SIZE {
            let packet_data = &buf.data_pool[offset..offset + len];

            // Передаем в smoltcp!
            // iface.process_packet(packet_data);

            // Освобождаем дескриптор
            core::ptr::write_volatile(&mut buf.descriptors[desc_idx].status, 0); // Free
        }

        // Обновляем tail
        let next_tail = (current_tail + 1) % RING_SIZE as u32;
        buf.tail.store(next_tail, Ordering::Release);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    sys::write(sys::STDOUT, b"[netd] Starting Network Daemon...\n");

    let mut iface = Interface::new(
        Config::new(smoltcp::wire::EthernetAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56])),
                                   &mut CubePhy,
                                   Instant::ZERO,
    );

    let mut sockets = SocketSet::new(vec![]);

    sys::write(sys::STDOUT, b"[netd] Stack initialized. Waiting for drivers...\n");

    loop {
        let timestamp = Instant::from_millis(sys::uptime_ms()); // Нужен такой syscall

        // Опрос интерфейса
        iface.poll(timestamp, &mut CubePhy, &mut sockets);

        // Тут обработка сокетов пользователей (IPC от csh/netsh)

        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys::debug("[netd] PANIC!\n");
    loop {}
}
