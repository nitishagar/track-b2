//! Shared synthetic-report generator for scale tests (B3/D5).
//! Synthesized content in the genuine `report_checks` layout; no golden data.

/// One minimal valid max block. Endpoint index and slack vary with `i`;
/// arrival/required/slack arithmetic stays consistent (hundredths of ns).
#[must_use]
pub fn synth_block(i: usize) -> String {
    let d_h = 10 + i64::try_from(i % 7).expect("small index");
    let arr_h = 65 + d_h;
    let req_h = 90;
    let slack_h = req_h - arr_h;
    debug_assert!(slack_h > 0);
    let f = |h: i64| format!("{:.2}", f64::from(i32::try_from(h).expect("small")) / 100.0);
    format!(
        "Startpoint: u_gen/ff{i} (rising edge-triggered flip-flop clocked by clk)\n\
         Endpoint: u_gen/g{i} (rising edge-triggered flip-flop clocked by clk)\n\
         Path Group: clk\n\
         Path Type: max\n\
         \n\
         Delay    Time   Description\n\
         ---------------------------------------------------\n\
         0.00    0.00   clock clk (rise edge)\n\
         0.00    0.00   clock source latency\n\
         0.01    0.01 ^ gbuf{i}/Z (CLKBUF_X1)\n\
         0.00    0.01 ^ u_gen/ff{i}/CK (DFF_X1)\n\
         0.64    0.65 ^ u_gen/ff{i}/Q (DFF_X1)\n\
         {d}    {a} ^ u_gen/g{i}/D (DFF_X1)\n\
         {a}   data arrival time\n\
         \n\
         1.00    1.00   clock clk (rise edge)\n\
         0.00    1.00   clock network delay (ideal)\n\
         -0.05    0.95   clock uncertainty\n\
         -0.05    0.90   library setup time\n\
         0.90   data required time\n\
         ---------------------------------------------------\n\
         0.90   data required time\n\
         -{a}   data arrival time\n\
         ---------------------------------------------------\n\
         {s}   slack (MET)\n",
        d = f(d_h),
        a = f(arr_h),
        s = f(slack_h),
    )
}

/// `n` concatenated blocks with distinct endpoints.
#[must_use]
pub fn synth_input(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        out.push_str(&synth_block(i));
        out.push('\n');
    }
    out
}
