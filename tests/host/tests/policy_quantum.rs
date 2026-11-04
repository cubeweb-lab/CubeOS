//! Хост-тесты load quantum и среднего загрузки — той части модели XNU,
//! которая заменяет фиксированный тик-квант.

use checks_host::policy::{
    self, load_avg_update, quantum_for, scale_quantum, warp_quantum, BAND_UTILITY, LOAD_DECAY,
    LOAD_FIXED, LOAD_SCALE_CAP, MAX_QUANTUM_TICKS, NBANDS, TS_BEST, TS_WORST,
};

#[test]
fn quantum_grows_with_load_and_is_capped() {
    let base = 2;
    let mut prev = 0;
    // От нулевой загрузки до 8 потоков на ядро квант монотонен не убывает.
    for l in 0..=8u32 {
        let q = scale_quantum(base, l * LOAD_FIXED);
        assert!(q >= 1, "quantum must never be zero (livelock guard)");
        assert!(q <= MAX_QUANTUM_TICKS);
        assert!(q >= prev, "quantum must not shrink as load grows");
        prev = q;
    }
    // При загрузке ровно 1.0 базовый load quantum сохраняется целиком.
    assert_eq!(scale_quantum(base, LOAD_FIXED), base);
    // Переполнение входа не должно ломать результат.
    assert_eq!(scale_quantum(base, u32::MAX), MAX_QUANTUM_TICKS);
}

#[test]
fn warp_shortens_slice_for_demoted_threads_but_never_to_zero() {
    let mut prev = MAX_QUANTUM_TICKS + 1;
    for slide in TS_BEST..=TS_WORST {
        let q = warp_quantum(4, slide);
        assert!(q >= 1, "warped quantum must stay >= 1");
        assert!(q <= prev, "warp must be non-increasing along the slide");
        prev = q;
    }
}

#[test]
fn every_band_always_gets_a_positive_slice() {
    // Квант = 0 означал бы вечную перепланировку на каждом тике.
    for band in 0..NBANDS {
        for slide in TS_BEST..=TS_WORST {
            for load in [0, LOAD_FIXED, 4 * LOAD_FIXED, LOAD_SCALE_CAP * LOAD_FIXED, u32::MAX] {
                let q = quantum_for(band, slide, load);
                assert!(q >= 1 && q <= MAX_QUANTUM_TICKS, "band {} slide {} load {}", band, slide, load);
            }
        }
    }
}

#[test]
fn interactive_slice_is_not_longer_than_background() {
    // Интерактивная полоса обязана уступать CPU чаще фоновой (отзывчивость).
    let idle_load = LOAD_FIXED;
    assert!(
        quantum_for(policy::BAND_INTERACTIVE, TS_BEST, idle_load)
            <= quantum_for(BAND_UTILITY, TS_BEST, idle_load)
    );
}

#[test]
fn load_average_decays_and_converges() {
    // Нет готовых -> среднее экспоненциально падает к нулю.
    let mut a = 8 * LOAD_FIXED;
    let mut steps = 0;
    while a > 0 && steps < 500 {
        a = load_avg_update(a, 0);
        steps += 1;
    }
    assert_eq!(a, 0, "load average must decay to zero");

    // Постоянная загрузка в 3 потока -> сходение к 3.0 в Q8. Целочисленное
    // усечение даёт систематическое занижение не больше чем на LOAD_DECAY.
    let mut a = 0;
    for _ in 0..400 {
        a = load_avg_update(a, 3);
    }
    assert!(
        a >= 3 * LOAD_FIXED - LOAD_DECAY && a <= 3 * LOAD_FIXED,
        "converged to load 3: {}",
        a
    );

    // Кап входа: 1000 готовых не должны «выстрелить» за шкалу масштабирования.
    assert!(load_avg_update(0, 1000) <= LOAD_SCALE_CAP * LOAD_FIXED);
    assert_eq!(load_avg_update(0, u32::MAX), load_avg_update(0, LOAD_SCALE_CAP));
}

#[test]
fn load_average_is_monotonic_in_input() {
    let a = load_avg_update(500, 1);
    let b = load_avg_update(500, 2);
    let c = load_avg_update(500, 4);
    assert!(a < b && b < c);
}
