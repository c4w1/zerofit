# zerofit-fueling

Rules-based carbohydrate and protein periodization for endurance training.
It takes a day's planned (or completed) sessions and returns a day plan:

- **daily targets**, scaled by that day's training load and raised for
  the next days' sessions (look-ahead);
- **per-meal targets**;
- **a timeline around each session**: pre-ride meal (split into a meal
  and a top-up when large), in-ride feeds every 30 minutes, recovery feeds;
  meals move around the sessions (an early ride's pre-ride meal is
  breakfast, an evening ride gets a post-ride dinner).

Times are on the quarter hour and grams are multiples of 5.

Every rule comes from published sports-nutrition consensus and is cited in
the doc comment where it is implemented. There is no learned model: every
number traces to a formula.

> **General guidance for healthy athletes, not medical or dietary advice.**

```rust
use zerofit_fueling::{Athlete, DayAhead, DayInput, MealSchedule, PlannedSession, day_plan};

let sessions = [PlannedSession::new(9 * 60, 180, 0.75)]; // 09:00, 3 h, IF 0.75
let tomorrow = [PlannedSession::new(9 * 60, 240, 0.70)]; // 4 h tomorrow at 09:00
let plan = day_plan(&DayInput {
    athlete: Athlete { body_mass_kg: 70.0, ftp_w: Some(250.0) },
    sessions: &sessions,
    ahead: &[DayAhead { sessions: &tomorrow, priority: None }],
    schedule: MealSchedule::default(),
})?;
// plan.carbs_g_per_kg, plan.raise ("Raised: 4 h ride tomorrow"),
// plan.protein_g, plan.entries (time, kind, carbs, protein)
```

## Rules

| Rule | Value | Source |
|---|---|---|
| Daily carbohydrate | light 3–5, moderate (~1 h) 5–7, high (1–3 h) 6–10, very high (4–5+ h) 8–12 g/kg | Thomas, Erdman & Burke 2016 (ACSM/AND/DC); Burke et al. 2011 (IOC) |
| Band label | from the final target (after look-ahead, rounded to 0.1 g/kg) by non-overlapping cut-offs: Light < 5, Moderate < 6.5, High < 9, Very high ≥ 9 g/kg, so the label always agrees with the number | this crate (`LoadBand::for_g_per_kg`) |
| Point within the range | piecewise linear in effective load (kJ/kg × intensity weight), anchors on the band boundaries | this crate; judgment call documented in `daily` |
| Look-ahead (the day before) | session > 90 min tomorrow: at least 5 g/kg rising to 7 at 4 h · ≥ 4 h: at least 8 · IF ≥ 0.80 for ≥ 45 min: at least 5.5 · A event > 90 min in 1–2 days: carbohydrate-load, 10 g/kg rising to 12 at 4.5 h; it only raises, and the reason is shown ("Raised: 4 h ride tomorrow") | Impey et al. 2018 ("fuel for the work required"); Thomas, Erdman & Burke 2016 (ACSM, loading 36–48 h before events > 90 min); Burke et al. 2011 |
| Daily protein | 1.2–2.0 g/kg, rising with load, and at least 0.3 g/kg per feeding | Thomas et al. 2016; Jäger et al. 2017 (ISSN) |
| Protein per feeding | equal shares of 0.3–0.4 g/kg; a protein snack fills a gap over 3.5 h when a share would pass 0.4 g/kg | Jäger et al. 2017 (ISSN); Moore et al. 2015; Areta et al. 2013 |
| Meal floors | a main meal gets at least max(0.5 g/kg, 10 % of the day) of carbohydrate, a snack half | this crate; judgment call documented in `plan` |
| Pre-ride | 1–4 g/kg, 1–4 h before (1 g/kg per hour of lead time, limited by wake time, rounded down to 0.5 h); above 2 g/kg, 75 % as a meal and 25 % as a top-up 1 h before | Thomas et al. 2016; Burke et al. 2011 |
| During | < 45 min: none · 45–75 min: 0–30 g/h or a mouth rinse · 75 min–2 h: 30–60 g/h · 2–2.5 h: 45–60 g/h · > 2.5 h: 60–90 g/h, glucose + fructose above 60 g/h; intensity picks the point | Jeukendrup 2014; Carter et al. 2004 |
| Recovery | next session < 8 h away: 1.0–1.2 g/kg/h for up to 4 h, within the day's budget; otherwise regular meals; a protein feed either way | Burke et al. 2011; Thomas et al. 2016; Moore et al. 2009 |

Intensity picks the point within each range, and so does duration through
the work done. The [daily-load index](src/daily.rs) weights work per kg by
`IF / 0.75`, because carbohydrate's share of the energy rises with
intensity (Romijn et al. 1993; van Loon et al. 2001).

## Guarantees

- **Monotone:** more duration, intensity or work never lowers the daily
  targets, the pre-ride meal, the in-ride rate or the recovery rate. This
  is a property test. The same holds for the days ahead: a longer,
  harder or higher-priority session tomorrow never lowers today's target
  (property test), and golden-day tests pin the demo cases (rest before a
  4 h ride, rest before rest, the day before an A race).
- **Consistent:** the timeline is sorted, times are multiples of 15 min
  and grams of 5 g, and both carbohydrate and protein sum to the target
  within 2.5 g (largest-remainder rounding) unless the fixed feeds exceed
  it, which the plan flags. Every meal meets its floors. In-ride feeds
  fall strictly inside the session.
- **Targets stay in their bands:** every daily target lies inside its
  load band's consensus range (unit test across loads).
- `#![no_std]` + `alloc`, no dependencies, panic-free under the same lints
  as the other zerofit crates.

## License

MIT OR Apache-2.0.
