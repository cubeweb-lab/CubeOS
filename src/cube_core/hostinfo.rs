// src/cube_core/hostinfo.rs
// Определение модели компьютера через SMBIOS (DMI)

use crate::zigfs::write_file;

/// Найти tag type 13 (SMBIOS) в Multiboot2 info и вернуть указатель на таблицы
unsafe fn find_smbios_tables(mb_info_addr: u32) -> Option<(*const u8, usize)> {
    let total_size = *(mb_info_addr as *const u32) as usize;
    let mut ptr = mb_info_addr as usize + 8; // пропускаем total_size + reserved
    let end = mb_info_addr as usize + total_size;

    while ptr + 8 <= end {
        let tag_type = *(ptr as *const u32);
        let tag_size = *(ptr as *const u32).add(1) as usize;

        if tag_type == 0 { break; }

        if tag_type == 13 {
            // SMBIOS tag: type(4) + size(4) + major(1) + minor(1) + reserved(6) + tables
            let tables_ptr = (ptr + 16) as *const u8;
            let tables_len = tag_size.saturating_sub(16);
            return Some((tables_ptr, tables_len));
        }

        // выравнивание по 8
        ptr += (tag_size + 7) & !7;
    }
    None
}

/// Найти Product Name в SMBIOS Type 1 (System Information)
unsafe fn find_product_name(tables: *const u8, len: usize) -> Option<[u8; 64]> {
    let mut ptr = tables;
    let end = tables.add(len);

    while ptr < end {
        // Проверяем, что хватает места на заголовок
        if ptr.add(4) > end { break; }

        let struct_type = *ptr;
        let struct_len = *ptr.add(1) as usize;

        if struct_len < 4 { break; }
        if ptr.add(struct_len) > end { break; }

        // Type 1 = System Information, offset 0x05 = Product Name (string index)
        if struct_type == 1 && struct_len >= 0x06 {
            let product_name_idx = *ptr.add(0x05);
            if product_name_idx != 0 {
                // Ищем строку с этим индексом
                // Строки идут после структуры, каждая NUL-terminated, потом двойной NUL
                let mut str_ptr = ptr.add(struct_len);
                let mut current_idx = 1u8;

                while str_ptr < end {
                    if *str_ptr == 0 {
                        // Конец строк в этой структуре
                        break;
                    }

                    if current_idx == product_name_idx {
                        // Нашли нужную строку
                        let mut result = [0u8; 64];
                        let mut i = 0;
                        let mut s = str_ptr;
                        while s < end && *s != 0 && i < 63 {
                            result[i] = *s;
                            i += 1;
                            s = s.add(1);
                        }
                        return Some(result);
                    }

                    // Пропускаем строку до NUL
                    while str_ptr < end && *str_ptr != 0 {
                        str_ptr = str_ptr.add(1);
                    }
                    if str_ptr < end { str_ptr = str_ptr.add(1); } // пропускаем NUL
                    current_idx += 1;
                }
            }
        }

        // Переходим к следующей структуре
        // Пропускаем formatted area
        let mut next = ptr.add(struct_len);
        // Пропускаем строки (до двойного NUL)
        while next < end {
            if *next == 0 {
                next = next.add(1);
                if next < end && *next == 0 {
                    next = next.add(1);
                    break;
                }
            } else {
                next = next.add(1);
            }
        }
        ptr = next;
    }

    None
}

/// Форматирует модель компьютера в строку
pub fn format_comp() -> ([u8; 64], usize) {
    let mut out = [0u8; 64];

    // Пытаемся получить из SMBIOS
    if let Some((tables, len)) = unsafe { find_smbios_tables(crate::MB_INFO_ADDR) } {
        if let Some(name) = unsafe { find_product_name(tables, len) } {
            let end = name.iter().position(|&b| b == 0).unwrap_or(64);
            if end > 0 {
                out[..end].copy_from_slice(&name[..end]);
                return (out, end);
            }
        }
    }

    // Fallback — generic
    let fallback = b"Unknown Computer";
    out[..fallback.len()].copy_from_slice(fallback);
    (out, fallback.len())
}

/// Пишет модель компьютера в /system/info/comp
pub fn write_comp_file(mb_info_addr: u32, start_dir: u32) -> bool {
    let (buf, len) = format_comp();
    let mut line = [0u8; 66];
    line[..len].copy_from_slice(&buf[..len]);
    line[len] = b'\n';

    write_file(start_dir, b"/system/info/comp", &line[..len + 1])
}
