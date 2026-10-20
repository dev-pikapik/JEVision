//! Laying out a very long test must stay fast and must not run out of stack.

use std::time::{Duration, Instant};

use jevision_core::{Flow, layout, parse};

/// A test of about `n` blocks in one long chain: every few blocks an If whose branches join
/// again, now and then a Repeat with a short body.
fn long_test(n: usize) -> String {
    let click = r#""type": "click", "target": {"point": {"x": 0.5, "y": 0.5}}, "button": "left""#;
    let mut blocks = Vec::new();
    let mut s = 0;
    while blocks.len() < n {
        let next = format!("s{}", s + 1);
        let at = format!("s{s}");
        match s % 4 {
            0 => blocks.push(format!(
                r#""{at}": {{"type": "if", "reference": {{"image": "refs/a.png"}}, "then": "{at}-then", "else": "{at}-else"}}, "{at}-then": {{{click}, "next": "{next}"}}, "{at}-else": {{{click}, "next": "{next}"}}"#
            )),
            1 => blocks.push(format!(
                r#""{at}": {{"type": "repeat", "times": 2, "body": "{at}-body", "next": "{next}"}}, "{at}-body": {{{click}}}"#
            )),
            _ => blocks.push(format!(r#""{at}": {{{click}, "next": "{next}"}}"#)),
        }
        s += 1;
    }
    blocks.push(format!(r#""s{s}": {{{click}}}"#));
    format!(
        r#"{{"format": 1, "name": "Long", "app": "com.example.App", "start": "s0", "blocks": {{{}}}}}"#,
        blocks.join(", ")
    )
}

/// The fastest of a few layouts of `flow`, so a busy machine skews the time as little as possible.
fn time_layout(flow: &Flow) -> Duration {
    (0..3)
        .map(|_| {
            let clock = Instant::now();
            let laid = layout(flow);
            let took = clock.elapsed();
            assert_eq!(laid.blocks.len(), flow.blocks.len());
            took
        })
        .min()
        .unwrap()
}

/// Sixteen times the blocks must take well under sixteen squared times as long: the time is
/// compared with a smaller test on the same machine and build, never with a fixed budget.
#[test]
fn a_very_long_test_lays_out_in_about_linear_time() {
    let small = parse(&long_test(1_000)).unwrap();
    let large = parse(&long_test(16_000)).unwrap();
    let (small_took, large_took) = (time_layout(&small), time_layout(&large));
    println!(
        "{} blocks: {small_took:?}, {} blocks: {large_took:?}",
        small.blocks.len(),
        large.blocks.len()
    );
    assert!(
        large_took < small_took * 64,
        "{} blocks took {large_took:?}, more than 64 times the {small_took:?} of {} blocks",
        large.blocks.len(),
        small.blocks.len()
    );
}
