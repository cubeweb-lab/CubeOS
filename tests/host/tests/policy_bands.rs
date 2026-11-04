//! Хост-тесты полос и time-slide (src/cube_core/sched/policy.rs):
//! уровни влезают в битмап, сдвиг не покидает полосу, полосы упорядочены.

use checks_host::policy::{
    band_name, band_of, demote, level_ok, promote, run_state, tslot_of, Qos, BAND_BACKGROUND,
    BAND_IDLE, BAND_INITIATED, BAND_INTERACTIVE, BAND_REALTIME, BAND_SYSTEM, BAND_UTILITY,
    LEVELS, NBANDS, SLIDES, TS_BEST, TS_DEFAULT, TS_WORST,
};

#[test]
fn every_run_state_fits_the_ready_bitmap() {
    // Старая версия использовала u128 и priority до 180 -> UB со сдвигом.
    for band in 0..NBANDS {
        for slide in TS_BEST..=TS_WORST {
            let rs = run_state(band, slide);
            assert!(level_ok(rs), "level {} out of range", rs);
            assert!(rs < 64, "run_state {} must fit a u64 bitmap", rs);
        }
    }
    assert_eq!(LEVELS, (NBANDS * SLIDES) as usize);
    assert!(LEVELS <= 64);
}

#[test]
fn run_state_roundtrips_band_and_slide() {
    for band in 0..NBANDS {
        for slide in TS_BEST..=TS_WORST {
            let rs = run_state(band, slide);
            assert_eq!(band_of(rs), band);
            assert_eq!(tslot_of(rs), slide);
        }
    }
}

#[test]
fn slide_moves_never_leave_the_band() {
    for band in 0..NBANDS {
        let mut s = TS_BEST;
        for _ in 0..(SLIDES * 3) {
            s = demote(s);
            assert_eq!(band_of(run_state(band, s)), band);
        }
        assert_eq!(s, TS_WORST);

        let mut s = TS_WORST;
        for _ in 0..(SLIDES * 3) {
            s = promote(s);
            assert_eq!(band_of(run_state(band, s)), band);
        }
        assert_eq!(s, TS_BEST);
    }
}

#[test]
fn any_level_of_higher_band_beats_every_level_of_lower_band() {
    // Худший слайд вышестоящей полосы обязан быть лучше лучшего слайда нижней:
    // иначе aging мог перевести Utility выше Interactive (баг прежней версии).
    let pairs = [
        (BAND_INTERACTIVE, BAND_INITIATED),
        (BAND_INITIATED, BAND_UTILITY),
        (BAND_UTILITY, BAND_BACKGROUND),
        (BAND_BACKGROUND, BAND_IDLE),
        (BAND_SYSTEM, BAND_INTERACTIVE),
        (BAND_REALTIME, BAND_SYSTEM),
    ];
    for (high, low) in pairs {
        assert!(run_state(high, TS_WORST) > run_state(low, TS_BEST));
    }
}

#[test]
fn run_state_clamps_out_of_range_inputs() {
    // Защита от порчи данных: заведомо невозможные полоса/slide не должны
    // выдавать уровень за пределами битмапа.
    assert!(level_ok(run_state(NBANDS + 5, TS_DEFAULT)));
    assert!(level_ok(run_state(BAND_IDLE, TS_WORST + 40)));
    assert_eq!(band_of(run_state(NBANDS + 5, TS_DEFAULT)), NBANDS - 1);
}

#[test]
fn qos_classes_map_to_ordered_distinct_bands() {
    assert!(Qos::Interactive.band() > Qos::Initiated.band());
    assert!(Qos::Initiated.band() > Qos::Utility.band());
    assert!(Qos::Utility.band() > Qos::Background.band());
    assert!(Qos::Background.band() > Qos::Idle.band());
    assert_eq!(Qos::Idle.band(), BAND_IDLE);
    let bands = [
        Qos::Interactive.band(),
        Qos::Initiated.band(),
        Qos::Utility.band(),
        Qos::Background.band(),
        Qos::Idle.band(),
    ];
    for b in bands.iter() {
        for c in bands.iter() {
            if b != c {
                assert!(b < c || b > c);
            }
        }
    }
}

#[test]
fn band_names_are_ascii_and_distinct() {
    // Вывод идёт в VGA/COM1 ASCII-шрифтом: не-ASCII в имени полосы сломал бы
    // таблицу планировщика на экране.
    let mut seen = Vec::new();
    for b in 0..NBANDS {
        let n = band_name(b);
        assert!(n.is_ascii(), "band name {:?} is not ascii", n);
        assert!(!seen.contains(&n), "duplicate band name {:?}", n);
        seen.push(n);
    }
}
