use rich_ext::layout::{allocate, Constraint};
fn fixed(n: usize) -> Constraint {
    Constraint {
        min: 0,
        max: None,
        preferred: Some(n),
        flex: 0,
    }
}
fn flex(min: usize, max: Option<usize>, weight: usize) -> Constraint {
    Constraint {
        min,
        max,
        preferred: None,
        flex: weight,
    }
}
#[test]
fn pressure_shrinks_preferences_then_minima_without_exceeding_viewport() {
    assert_eq!(
        allocate(10, &[fixed(8), fixed(8)]).unwrap().sizes,
        vec![5, 5]
    );
    assert_eq!(
        allocate(100, &[fixed(80), fixed(80)]).unwrap().sizes,
        vec![50, 50]
    );
    let a = allocate(10, &[flex(8, None, 1), flex(8, None, 1)]).unwrap();
    assert_eq!(a.sizes, vec![5, 5]);
    assert_eq!(a.relaxed, vec![0, 1]);
    assert_eq!(
        allocate(0, &[fixed(8), fixed(8)]).unwrap().sizes,
        vec![0, 0]
    );
    assert_eq!(
        allocate(5, &[fixed(8), fixed(8)]).unwrap().sizes,
        vec![3, 2]
    );
}
#[test]
fn caps_leave_padding_and_invalid_constraints_are_errors() {
    let a = allocate(10, &[flex(0, Some(3), 1), flex(0, Some(4), 1)]).unwrap();
    assert_eq!((a.sizes, a.padding), (vec![3, 4], 3));
    assert!(allocate(10, &[flex(5, Some(3), 1)]).is_err());
    assert!(allocate(10, &[flex(0, None, 0)]).is_err());
    assert!(allocate(10, &[Constraint { min: 5, ..fixed(3) }]).is_err());
}
#[test]
fn large_values_and_rounding_conserve_total() {
    let a = allocate(usize::MAX, &[fixed(usize::MAX), fixed(usize::MAX)]).unwrap();
    assert_eq!(a.sizes.iter().sum::<usize>(), usize::MAX);
    for total in 0..129 {
        for constraints in [
            vec![],
            vec![fixed(8), flex(3, Some(40), 5)],
            vec![flex(0, None, usize::MAX), flex(0, None, usize::MAX)],
        ] {
            let a = allocate(total, &constraints).unwrap();
            assert_eq!(a.sizes.iter().sum::<usize>() + a.padding, total);
            assert_eq!(a, allocate(total, &constraints).unwrap());
        }
    }
}
