// src/libs/cube_syscall/src/net.rs
use core::sync::atomic::{AtomicU32, Ordering};

pub const RING_SIZE: usize = 64;
pub const PACKET_DATA_SIZE: usize = 2048; // Максимальный размер одного пакета

#[repr(C)]
pub struct NetRingBuffer {
    // Индексы кольца
    pub head: AtomicU32, // Пишет драйвер
    pub tail: AtomicU32, // Читает netd

    // Массив дескрипторов
    pub descriptors: [NetPacketDesc; RING_SIZE],

    // Непосредственно данные пакетов (плоский буфер)
    // Каждый дескриптор указывает на смещение в этом массиве
    pub data_pool: [u8; RING_SIZE * PACKET_DATA_SIZE],
}

#[repr(C)]
pub struct NetPacketDesc {
    pub len: u16,      // Длина полезных данных
    pub status: u16,   // 0: Free, 1: Ready (for netd), 2: Processed (for driver)
    pub offset: u32,   // Смещение начала данных в data_pool
}

impl NetRingBuffer {
    pub const fn new() -> Self {
        Self {
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            descriptors: [NetPacketDesc { len: 0, status: 0, offset: 0 }; RING_SIZE],
            data_pool: [0; RING_SIZE * PACKET_DATA_SIZE],
        }
    }
}
