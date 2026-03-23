//! Tests for player experience

use steel_core::player::experience::Experience;

#[test]
fn level_round_trips_and_progress_invariants() {
    for level in 0..100 {
        let at_boundary = Experience::total_points_at_level(level);
        let xp = Experience::new(at_boundary);

        assert_eq!(
            xp.level(),
            level,
            "level({at_boundary} points) = {}, expected {level}",
            xp.level()
        );
        assert_eq!(
            xp.points(),
            0,
            "points({at_boundary} points) = {}, expected 0 at level {level} boundary",
            xp.points()
        );
        assert!(
            xp.progress().abs() < f64::EPSILON,
            "progress({at_boundary} points) = {}, expected 0.0 at level {level} boundary",
            xp.progress()
        );

        if level > 0 {
            let xp = Experience::new(at_boundary + 1);
            assert_eq!(
                xp.level(),
                level,
                "level({} points) = {}, expected {level}",
                at_boundary + 1,
                xp.level()
            );
            assert_eq!(xp.points(), 1);
            let p = xp.progress();
            assert!(
                p > 0.0 && p < 1.0,
                "progress({} points) = {p}, expected in (0, 1) at level {level}",
                at_boundary + 1
            );
        }

        if at_boundary > 0 {
            let xp = Experience::new(at_boundary - 1);
            assert_eq!(
                xp.level(),
                level - 1,
                "level({} points) = {}, expected {} (one below level {level} boundary)",
                at_boundary - 1,
                xp.level(),
                level - 1
            );
        }
    }
}

#[test]
fn points_for_level_known_values() {
    let expected = [
        (0, 7),
        (1, 9),
        (14, 35),
        (15, 37),
        (16, 42),
        (29, 107),
        (30, 112),
        (31, 121),
        (50, 292),
    ];
    for (level, points) in expected {
        let actual = Experience::points_for_level(level);
        assert_eq!(
            actual, points,
            "points_for_level({level}) = {actual}, expected {points}"
        );
    }

    for level in 0..100 {
        let curr = Experience::points_for_level(level);
        let next = Experience::points_for_level(level + 1);
        assert!(
            next >= curr,
            "points_for_level not monotonic: level {level} = {curr}, level {} = {next}",
            level + 1
        );
    }
}

#[test]
fn add_and_subtract_points() {
    for start in [0, 50, 315, 316, 1000, 1507, 1508, 5000] {
        let mut xp = Experience::new(start);
        xp.add_points(100);
        assert_eq!(
            xp.total_points(),
            start + 100,
            "from {start}: add 100 -> {}, expected {}",
            xp.total_points(),
            start + 100
        );

        xp.add_points(-100);
        assert_eq!(
            xp.total_points(),
            start,
            "from {}: subtract 100 -> {}, expected {start}",
            start + 100,
            xp.total_points()
        );
    }

    for start in [0, 1, 50, 1000] {
        let mut xp = Experience::new(start);
        xp.add_points(-999_999);
        assert_eq!(
            xp.total_points(),
            0,
            "from {start}: add -999999 -> {}, expected 0",
            xp.total_points()
        );
    }
}

#[test]
fn add_levels_preserves_progress() {
    for base_level in [0, 5, 15, 29, 30, 50] {
        let base = Experience::total_points_at_level(base_level);
        let half_points = Experience::points_for_level(base_level) / 2;
        let xp = Experience::new(base + half_points);
        let progress_before = xp.progress();

        for delta in [1, 5, 10, 20] {
            let mut xp_copy = xp;
            xp_copy.add_levels(delta);
            let target = base_level + delta;
            assert_eq!(
                xp_copy.level(),
                target,
                "add_levels({delta}) from level {base_level}: got level {}, expected {target}",
                xp_copy.level()
            );
            let progress_after = xp_copy.progress();
            let tolerance = 0.5 / f64::from(Experience::points_for_level(target)) + f64::EPSILON;
            assert!(
                (progress_after - progress_before).abs() < tolerance,
                "add_levels({delta}) from level {base_level}: progress {progress_before:.4} -> {progress_after:.4}, \
                     delta {:.4} exceeds tolerance {tolerance:.4} (points_for_level({target}) = {})",
                (progress_after - progress_before).abs(),
                Experience::points_for_level(target)
            );
        }
    }
}

#[test]
fn set_levels_preserves_progress() {
    for (from, to) in [(5, 20), (20, 5), (14, 16), (29, 31), (50, 10)] {
        let base = Experience::total_points_at_level(from);
        let half = Experience::points_for_level(from) / 2;
        let mut xp = Experience::new(base + half);
        let progress_before = xp.progress();

        xp.set_levels(to);

        assert_eq!(
            xp.level(),
            to,
            "set_levels({to}) from level {from}: got level {}, expected {to}",
            xp.level()
        );
        let progress_after = xp.progress();
        let tolerance = 0.5 / f64::from(Experience::points_for_level(to)) + f64::EPSILON;
        assert!(
            (progress_after - progress_before).abs() < tolerance,
            "set_levels({to}) from level {from}: progress {progress_before:.4} -> {progress_after:.4}, \
                 delta {:.4} exceeds tolerance {tolerance:.4} (points_for_level({to}) = {})",
            (progress_after - progress_before).abs(),
            Experience::points_for_level(to)
        );
    }
}

#[test]
fn set_points_valid_and_invalid() {
    for level in [0, 1, 10, 15, 20, 30, 50] {
        let max = Experience::points_for_level(level);

        let mut xp = Experience::new(Experience::total_points_at_level(level));
        assert!(xp.set_points(0).is_ok(), "level {level}: rejected 0");
        assert_eq!(xp.points(), 0);

        assert!(
            xp.set_points(max - 1).is_ok(),
            "level {level}: rejected {max}-1 (max-1)",
        );
        assert_eq!(
            xp.points(),
            max - 1,
            "level {level}: set_points(max-1) -> points = {}, expected {}",
            xp.points(),
            max - 1
        );

        assert!(
            xp.set_points(max).is_err(),
            "level {level}: accepted {max} (max), which would level up"
        );
        assert!(xp.set_points(-1).is_err(), "level {level}: accepted -1");
    }
}

#[test]
fn set_progress_clamps_and_applies() {
    for level in [0, 10, 15, 30, 50] {
        let mut xp = Experience::new(Experience::total_points_at_level(level));

        xp.set_progress(0.5);
        assert_eq!(
            xp.level(),
            level,
            "set_progress(0.5) at level {level}: level changed to {}",
            xp.level()
        );
        let expected = (f64::from(Experience::points_for_level(level)) * 0.5).round() as i32;
        assert_eq!(
            xp.points(),
            expected,
            "set_progress(0.5) at level {level}: points = {}, expected {expected}",
            xp.points()
        );

        xp.set_progress(-5.0);
        assert_eq!(
            xp.points(),
            0,
            "set_progress(-5.0) at level {level}: points = {}, expected 0",
            xp.points()
        );

        xp.set_progress(999.0);
        assert_eq!(
            xp.level(),
            level,
            "set_progress(999.0) at level {level}: level changed to {}",
            xp.level()
        );
    }
}

#[test]
fn negative_inputs_clamp_to_zero() {
    for level in [-1, -10, -100] {
        let tp = Experience::total_points_at_level(level);
        assert_eq!(tp, 0, "total_points_at_level({level}) = {tp}, expected 0");

        let pfl = Experience::points_for_level(level);
        assert_eq!(pfl, 0, "points_for_level({level}) = {pfl}, expected 0");
    }

    let mut xp = Experience::new(Experience::total_points_at_level(5));
    xp.add_levels(-100);
    assert_eq!(
        xp.total_points(),
        0,
        "add_levels(-100) from level 5: total_points = {}, expected 0",
        xp.total_points()
    );

    let mut xp = Experience::new(100);
    xp.set_levels(-5);
    assert_eq!(
        xp.total_points(),
        0,
        "set_levels(-5) from 100 points: total_points = {}, expected 0",
        xp.total_points()
    );
}

#[test]
fn overflow_does_not_panic() {
    let mut xp = Experience::new(i32::MAX - 10);
    xp.add_points(100);
    assert_eq!(
        xp.total_points(),
        i32::MAX,
        "saturating add near MAX: got {}, expected {}",
        xp.total_points(),
        i32::MAX
    );

    let mut xp = Experience::new(0);
    xp.add_points(i32::MIN);
    assert_eq!(
        xp.total_points(),
        0,
        "add MIN to 0: got {}, expected 0",
        xp.total_points()
    );
}
