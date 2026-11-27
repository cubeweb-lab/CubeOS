use cube_syscall::net::{NetRingBuffer, RING_SIZE, PACKET_DATA_SIZE};

const SHARED_ADDR: u64 = 0x4000_0000;

fn send_packet(data: &[u8]) {
    unsafe {
        let buf = &*(SHARED_ADDR as *const NetRingBuffer);

        let current_head = buf.head.load(Ordering::Relaxed);
        let next_head = (current_head + 1) % RING_SIZE as u32;
        let current_tail = buf.tail.load(Ordering::Acquire);

        // Если буфер полон, дропаем пакет
        if next_head == current_tail {
            return;
        }

        let desc_idx = current_head as usize;
        let desc = &buf.descriptors[desc_idx];

        // Копируем данные в pool
        let offset = desc_idx * PACKET_DATA_SIZE;
        let dst = &mut buf.data_pool[offset..offset + data.len()];
        dst.copy_from_slice(data);

        // Заполняем дескриптор
        // Внимание: в реальной жизни нужно использовать UnsafeCell или мутабельный указатель
        // Так как мы замапили память как Read/Write, мы можем использовать ptr::write
        core::ptr::write_volatile(&mut buf.descriptors[desc_idx].len, data.len() as u16);
        core::ptr::write_volatile(&mut buf.descriptors[desc_idx].status, 1); // Ready
        core::ptr::write_volatile(&mut buf.descriptors[desc_idx].offset, offset as u32);

        // Обновляем head
        buf.head.store(next_head, Ordering::Release);
    }
}
