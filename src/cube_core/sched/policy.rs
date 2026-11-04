// src/cube_core/sched/policy.rs
//
// Чистая политика планирования в духе macOS/XNU: никаких MMIO, прерываний и
// аллокатора, поэтому модуль компилируется и в ядро, и в хост-тесты
// (tests/host) — все правила покрыты unit-тестами.
//
// Модель повторяет primary-планировщик XNU (os/sched.h, sched_prim.c):
//  * фиксированные приоритетные ПОЛОСЫ — XNU throttle bands (TH_IDLE / TH_UTIL /
//    TH_USER / TH_UGEEK / TH_USER_INIT, плюс системная и real-time полосы);
//  * внутри полосы поток занимает один из SLIDES уровней «time slide»
//    (XNU: timeshare-полоса и её timeslide[]): поток, рано уходящий в блок,
//    поднимается вверх; поток, сжёгший весь отрезок, — опускается. Сдвиг
//    никогда не покидает пределы полосы, поэтому полосы нельзя «проткнуть»;
//  * длительность отрезка — не константа, а load quantum, масштабируемый по
//    экспоненциально затухающему среднему загрузки CPU (XNU loadquantum /
//    runquantum) и укорачиваемый для опустившегося потока (XNU warp);
//  * вытеснение событийное и ограничено по частоте: XNU sched_should_preempt()
//    вместе с гардами dup_preempt и runaway.

/// Уровней «time slide» внутри одной полосы (XNU: 8 timeslice-уровней).
pub const SLIDES: u32 = 8;
pub const TS_BEST: u32 = 0;
/// Старт в середине скользящей шкалы: потоку есть куда и расти, и падать.
pub const TS_DEFAULT: u32 = 3;
pub const TS_WORST: u32 = SLIDES - 1;

pub const NBANDS: u32 = 7;
/// Всего уровней планирования = 56 < 64, поэтому ready-битмап — u64, и сдвиг
/// не может выйти за его границы (в старой версии priority: u8 доходил до 180).
pub const LEVELS: usize = (NBANDS * SLIDES) as usize;

pub const BAND_IDLE: u32 = 0;
pub const BAND_BACKGROUND: u32 = 1;
pub const BAND_UTILITY: u32 = 2;
pub const BAND_INITIATED: u32 = 3;
pub const BAND_INTERACTIVE: u32 = 4;
pub const BAND_SYSTEM: u32 = 5;
pub const BAND_REALTIME: u32 = 6;

pub const fn band_name(band: u32) -> &'static str {
    match band {
        BAND_IDLE => "idle",
        BAND_BACKGROUND => "background",
        BAND_UTILITY => "utility",
        BAND_INITIATED => "initiated",
        BAND_INTERACTIVE => "interactive",
        BAND_SYSTEM => "system",
        BAND_REALTIME => "realtime",
        _ => "unknown",
    }
}

/// Класс обслуживания — как QoS в macOS (UserInteractive / UserInitiated /
/// Utility / Background / UtilityIdle). Номера полос не меняются, чтобы не
/// ломать существующие вызовы sched::spawn(.., Qos::X, ..).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Qos {
    Interactive,
    Initiated,
    Utility,
    Background,
    Idle,
}

impl Qos {
    pub const fn band(self) -> u32 {
        match self {
            Qos::Interactive => BAND_INTERACTIVE,
            Qos::Initiated   => BAND_INITIATED,
            Qos::Utility     => BAND_UTILITY,
            Qos::Background  => BAND_BACKGROUND,
            Qos::Idle        => BAND_IDLE,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Qos::Interactive => "Interactive",
            Qos::Initiated   => "Initiated",
            Qos::Utility     => "Utility",
            Qos::Background  => "Background",
            Qos::Idle        => "Idle",
        }
    }
}

// ===== run state: одно число задаёт и полосу, и slide =====

/// Старшие биты — полоса, младшие — slide (0 = лучший). Чем больше значение,
/// тем раньше поток уходит с вершины очереди.
pub const fn run_state(band: u32, tslot: u32) -> u32 {
    let b = if band >= NBANDS { NBANDS - 1 } else { band };
    let s = if tslot > TS_WORST { TS_WORST } else { tslot };
    b * SLIDES + (TS_WORST - s)
}

pub const fn band_of(rs: u32) -> u32 {
    rs / SLIDES
}

pub const fn tslot_of(rs: u32) -> u32 {
    TS_WORST - (rs % SLIDES)
}

pub const fn level_ok(rs: u32) -> bool {
    rs < LEVELS as u32
}

// ===== Load quantum: длительность отрезка зависит от загрузки CPU =====

/// Базовый load quantum в тиках (1 тик = 10 мс при 100 Гц). В XNU loadquantum
/// по умолчанию 20 мс; интерактивные полосы укорочены ради отзывчивости,
/// фоновые удлинены, чтобы реже платить за переключение контекста.
pub const fn loadquantum_ticks(band: u32) -> u32 {
    match band {
        BAND_IDLE => 1,
        BAND_BACKGROUND => 4,
        BAND_UTILITY => 2,
        BAND_INITIATED => 2,
        BAND_INTERACTIVE => 1,
        _ => 1,
    }
}

/// Q8: 256 == одна готовая к исполнению задача в среднем.
pub const LOAD_FIXED: u32 = 256;
/// Экспоненциальное затухание за ~1 с (XNU sched_load_avg_tick).
pub const LOAD_DECAY: u32 = 10;
/// Поток считается «голодающим» после 300 мс в очереди ожидания.
pub const STARVE_TICKS: u32 = 30;
/// Сколько slide-уровней может отвоевать aging (XNU TH_BORROWED_TIME).
pub const STARVE_BOOST_MAX: u32 = 2;
/// Поток, удерживающий CPU без смены, — XNU runaway.
pub const RUNAWAY_TICKS: u32 = 100;
/// XNU dup_preempt_count: не стричь один и тот же поток подряд.
pub const DUP_PREEMPT_MAX: u32 = 3;
/// Потолок кванта: длиннее 40 мс интерактивность уже не сохраняется.
pub const MAX_QUANTUM_TICKS: u32 = 4;
/// Каплинг среднего загрузки для масштаба кванта (8 потоков на ядро).
pub const LOAD_SCALE_CAP: u32 = 8;

/// Экспоненциально затухающее среднее числа готовых потоков в Q8
/// (XNU sched_load_avg_tick). Целочисленное усечение занижает предел не более
/// чем на LOAD_DECAY (для 3.0 это 768 -> 759), что для масштаба кванта несущественно.
pub const fn load_avg_update(prev: u32, runnable: u32) -> u32 {
    let capped = if runnable > LOAD_SCALE_CAP { LOAD_SCALE_CAP } else { runnable };
    (prev * (LOAD_DECAY - 1) + capped * LOAD_FIXED) / LOAD_DECAY
}

/// На лёгкой загрузке квант остаётся коротким (отзывчивость); при росте
/// среднего загрузки растёт, чтобы окупить стоимость переключения.
pub const fn scale_quantum(base_ticks: u32, load_fixed: u32) -> u32 {
    let capped =
        if load_fixed > LOAD_SCALE_CAP * LOAD_FIXED { LOAD_SCALE_CAP * LOAD_FIXED } else { load_fixed };
    let q = base_ticks * (LOAD_FIXED + capped) / (2 * LOAD_FIXED);
    if q < 1 {
        1
    } else if q > MAX_QUANTUM_TICKS {
        MAX_QUANTUM_TICKS
    } else {
        q
    }
}

/// XNU warp: опустившийся в timeshare-slide поток получает укороченный отрезок,
/// чтобы быстрее отдать CPU остальным.
pub const fn warp_quantum(q: u32, tslot: u32) -> u32 {
    let s = if tslot > 3 { 3 } else { tslot };
    let v = q >> s;
    if v < 1 {
        1
    } else {
        v
    }
}

pub const fn quantum_for(band: u32, tslot: u32, load_fixed: u32) -> u32 {
    warp_quantum(scale_quantum(loadquantum_ticks(band), load_fixed), tslot)
}

// ===== Решения о вытеснении и сдвигах по slide =====

pub const fn promote(tslot: u32) -> u32 {
    if tslot <= TS_BEST {
        TS_BEST
    } else {
        tslot - 1
    }
}

pub const fn demote(tslot: u32) -> u32 {
    if tslot >= TS_WORST {
        TS_WORST
    } else {
        tslot + 1
    }
}

/// Низший slide, до которого aging имеет право поднимать поток.
pub const fn boost_floor(default_slide: u32, cap: u32) -> u32 {
    if default_slide > cap {
        default_slide - cap
    } else {
        TS_BEST
    }
}

/// Aging ограничен снизу, поэтому повышение никогда не покидает полосу
/// (в старой версии Utility со временем обгонял Interactive).
pub const fn slide_after_wait(tslot: u32, waited: u32, floor: u32) -> u32 {
    if waited < STARVE_TICKS || tslot <= floor {
        tslot
    } else {
        tslot - 1
    }
}

/// Эквивалент XNU sched_should_preempt(): вытесняем, если отрезок текущего
/// потока истёк либо если кандидат из более высокой полосы/лучшего slide.
///
/// Порядок условий важен:
///  * idle никогда не держит CPU;
///  * истёкший отрезок отдаёт CPU ВСЕГДА — иначе поток с quantum_left == 0
///    залип бы на процессоре навсегда (это и поймал хост-тест);
///  * вытеснение «по приоритету» ограничено dup_preempt (XNU dup_preempt_count:
///    поток, которого стригут подряд, добирает отрезок — гасит пинг-понг).
pub const fn should_preempt(cand_rs: u32, cur_rs: u32, cur_quantum: u32, cur_is_idle: bool, dup_preempt: u32) -> bool {
    if cur_is_idle || cur_quantum == 0 {
        return true;
    }
    cand_rs > cur_rs && dup_preempt < DUP_PREEMPT_MAX
}

pub const fn runaway_expired(run_streak: u32) -> bool {
    run_streak >= RUNAWAY_TICKS
}

