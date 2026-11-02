//! Easings against Textual's (#814): its `_easing.py` formulas at 0.25,
//! 0.5 and 0.75, computed by Python from those formulas (Textual 3.0.0).

use intuituive::Easing;

const GOLDEN: [(Easing, [f64; 3]); 30] = [
    (
        Easing::InSine,
        [0.07612046748871326, 0.2928932188134524, 0.6173165676349102],
    ),
    (
        Easing::InOutSine,
        [0.1464466094067262, 0.49999999999999994, 0.8535533905932737],
    ),
    (
        Easing::OutSine,
        [0.3826834323650898, 0.7071067811865475, 0.9238795325112867],
    ),
    (Easing::InQuad, [0.0625, 0.25, 0.5625]),
    (Easing::InOutQuad, [0.125, 0.5, 0.875]),
    (Easing::OutQuad, [0.4375, 0.75, 0.9375]),
    (Easing::InCubic, [0.015625, 0.125, 0.421875]),
    (Easing::InOutCubic, [0.0625, 0.5, 0.9375]),
    (Easing::OutCubic, [0.578125, 0.875, 0.984375]),
    (Easing::InQuart, [0.00390625, 0.0625, 0.31640625]),
    (Easing::InOutQuart, [0.03125, 0.5, 0.96875]),
    (Easing::OutQuart, [0.68359375, 0.9375, 0.99609375]),
    (Easing::InQuint, [0.0009765625, 0.03125, 0.2373046875]),
    (Easing::InOutQuint, [0.015625, 0.5, 0.984375]),
    (Easing::OutQuint, [0.7626953125, 0.96875, 0.9990234375]),
    (
        Easing::InExpo,
        [0.005524271728019903, 0.03125, 0.1767766952966369],
    ),
    (Easing::InOutExpo, [0.015625, 0.5, 0.984375]),
    (
        Easing::OutExpo,
        [0.8232233047033631, 0.96875, 0.99447572827198],
    ),
    (
        Easing::InCirc,
        [0.031754163448145745, 0.1339745962155614, 0.3385621722338523],
    ),
    (
        Easing::InOutCirc,
        [0.0669872981077807, 0.5, 0.9330127018922193],
    ),
    (
        Easing::OutCirc,
        [0.6614378277661477, 0.8660254037844386, 0.9682458365518543],
    ),
    (
        Easing::InBack,
        [
            -0.06413656250000001,
            -0.08769750000000004,
            0.1825903124999998,
        ],
    ),
    (Easing::InOutBack, [-0.09968184375, 0.5, 1.09968184375]),
    (
        Easing::OutBack,
        [0.8174096875000002, 1.0876975, 1.0641365625],
    ),
    (
        Easing::InElastic,
        [
            -0.005524271728019903,
            -0.015625000000000045,
            0.08838834764831832,
        ],
    ),
    (
        Easing::InOutElastic,
        [0.011969444423734044, 0.5, 0.988030555576266],
    ),
    (
        Easing::OutElastic,
        [0.9116116523516816, 1.015625, 1.00552427172802],
    ),
    (Easing::InBounce, [0.02734375, 0.234375, 0.52734375]),
    (Easing::InOutBounce, [0.1171875, 0.5, 0.8828125]),
    (Easing::OutBounce, [0.47265625, 0.765625, 0.97265625]),
];

#[test]
fn every_easing_starts_at_zero_and_ends_at_one() {
    for easing in Easing::ALL {
        assert!(easing.at(0.0).abs() < 1e-12, "{easing:?} at 0");
        assert!((easing.at(1.0) - 1.0).abs() < 1e-12, "{easing:?} at 1");
    }
}

#[test]
fn easings_match_textuals_values() {
    for (easing, values) in GOLDEN {
        for (t, want) in [0.25, 0.5, 0.75].into_iter().zip(values) {
            let got = easing.at(t);
            assert!(
                (got - want).abs() < 1e-9,
                "{easing:?} at {t}: {got} != {want}"
            );
        }
    }
}

#[test]
fn the_old_names_are_the_cubic_ones() {
    for t in [0.1, 0.4, 0.6, 0.9] {
        assert_eq!(Easing::EaseIn.at(t), Easing::InCubic.at(t));
        assert_eq!(Easing::EaseOut.at(t), Easing::OutCubic.at(t));
        assert_eq!(Easing::EaseInOut.at(t), Easing::InOutCubic.at(t));
    }
}
