//! Хост-тесты решений о вытеснении, aging и runaway — аналогов
//! sched_should_preempt()/dup_preempt_count()/runaway из XNU sched_prim.c.

use checks_host::policy::{
    self, band_of, boost_floor, run_state, slide_after_wait, should_preempt,
    BAND_BACKGROUND, BAND_IDLE, BAND_INTERACTIVE, BAND_UTILITY, DUP_PREEMPT_MAX, RUNAWAY_TICKS,
    STARVE_BOOST_MAX, STARVE_TICKS, TS_BEST, TS_DEFAULT, TS_WORST,
};

#[test]
fn idle_must_never_hold_the_cpu() {
    let idle_rs = run_state(BAND_IDLE, TS_WORST);
    // Даже «бесконечный» квант idle не защищает его от вытеснения.
    assert!(should_preempt(idle_rs, idle_rs, u32::MAX, true, 0));
    assert!(should_preempt(
        run_state(BAND_UTILITY, TS_WORST),
        idle_rs,
        u32::MAX,
        true,
        DUP_PREEMPT_MAX
    ));
}

#[test]
fn higher_band_preempts_lower_regardless_of_quantum() {
    let hi = run_state(BAND_INTERACTIVE, TS_DEFAULT);
    let lo = run_state(BAND_UTILITY, TS_DEFAULT);
    assert!(should_preempt(hi, lo, 5, false, 0));
    // Обратный порядок — только когда отрезок текущего истёк.
    assert!(!should_preempt(lo, hi, 5, false, 0));
    assert!(should_preempt(lo, hi, 0, false, 0));
}

#[test]
fn same_level_with_quantum_left_is_not_preempted() {
    let same = run_state(BAND_UTILITY, TS_DEFAULT);
    assert!(!should_preempt(same, same, 2, false, 0));
    assert!(should_preempt(same, same, 0, false, 0));
}

#[test]
fn dup_preempt_guard_stops_thrashing() {
    let hi = run_state(BAND_INTERACTIVE, TS_DEFAULT);
    let lo = run_state(BAND_UTILITY, TS_DEFAULT);
    assert!(should_preempt(hi, lo, 3, false, DUP_PREEMPT_MAX - 1));
    // После DUP_PREEMPT_MAX подряд стрижек поток добирает отрезок сам.
    assert!(!should_preempt(hi, lo, 3, false, DUP_PREEMPT_MAX));
    // ...кроме случая, когда отрезок реально выбран до конца.
    assert!(should_preempt(hi, lo, 0, false, DUP_PREEMPT_MAX + 5));
}

#[test]
fn starvation_boost_is_bounded_and_band_confined() {
    let floor = boost_floor(TS_DEFAULT, STARVE_BOOST_MAX);
    assert_eq!(floor, TS_DEFAULT - STARVE_BOOST_MAX);

    // До STARVE_TICKS повышения нет.
    assert_eq!(slide_after_wait(TS_WORST, STARVE_TICKS - 1, floor), TS_WORST);
    // После — есть, но не выше floor.
    assert_eq!(slide_after_wait(TS_WORST, STARVE_TICKS, floor), TS_WORST - 1);
    assert_eq!(slide_after_wait(floor, STARVE_TICKS * 10, floor), floor);

    // Boost не уводит в чужую полосу: самый разогнанный Utility не обгоняет
    // самый худший Interactive (прежняя версия это ломала).
    let boosted = slide_after_wait(TS_WORST, u32::MAX, floor);
    assert!(run_state(BAND_UTILITY, boosted) < run_state(BAND_INTERACTIVE, TS_WORST));
    assert_eq!(band_of(run_state(BAND_UTILITY, boosted)), BAND_UTILITY);

    // Фоновая полоса тоже не может вылезти в Utility.
    let bg_boosted = slide_after_wait(TS_WORST, u32::MAX, floor);
    assert!(run_state(BAND_BACKGROUND, bg_boosted) < run_state(BAND_UTILITY, TS_WORST));
}

#[test]
fn boost_floor_never_leaves_the_slide_range() {
    assert_eq!(boost_floor(0, 5), TS_BEST);
    assert_eq!(boost_floor(1, 5), TS_BEST);
    assert_eq!(boost_floor(TS_DEFAULT, 0), TS_DEFAULT);
    assert!(boost_floor(TS_DEFAULT, STARVE_BOOST_MAX) >= TS_BEST);
}

#[test]
fn runaway_trips_after_about_one_second_of_cpu() {
    assert_eq!(RUNAWAY_TICKS, 100);
    assert!(!policy::runaway_expired(RUNAWAY_TICKS - 1));
    assert!(policy::runaway_expired(RUNAWAY_TICKS));
    assert!(policy::runaway_expired(RUNAWAY_TICKS + 1000));

    // runaway помечает поток exceded, а с истёкшим квантом вытеснение разрешено
    // даже вопреки dup_preempt-гарду.
    let same = run_state(BAND_UTILITY, TS_DEFAULT);
    assert!(should_preempt(same, same, 0, false, DUP_PREEMPT_MAX + 10));
}

#[test]
fn preemption_decision_is_total_for_valid_levels() {
    // Ни одна комбинация не должна паниковать/зависать: функция чистая.
    for a in 0..BAND_INTERACTIVE + 1 {
        for b in 0..BAND_INTERACTIVE + 1 {
            for q in [0u32, 1, 4, u32::MAX] {
                for dup in [0u32, DUP_PREEMPT_MAX, DUP_PREEMPT_MAX + 1] {
                    let _ = should_preempt(run_state(a, TS_DEFAULT), run_state(b, TS_WORST), q, false, dup);
                }
            }
        }
    }
}
