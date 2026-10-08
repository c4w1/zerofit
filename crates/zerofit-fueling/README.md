# zerofit-fueling

Rules-based carbohydrate and protein periodization for endurance training.
It takes a day's planned (or completed) sessions and returns a day plan:

- **daily targets**, scaled by that day's training load;
- **per-meal targets**;
- **a timeline around each session**: pre-ride meal, in-ride feeds every
  20 minutes, recovery feeds.

Every rule comes from published sports-nutrition consensus and is cited in
the doc comment where it is implemented. There is no learned model: every
number traces to a formula.

> **General guidance for healthy athletes, not medical or dietary advice.**

```rust
use zerofit_fueling::{Athlete, DayInput, MealSchedule, PlannedSession, day_plan};

let sessions = [PlannedSession::new(9 * 60, 180, 0.75)]; // 09:00, 3 h, IF 0.75
let plan = day_plan(&DayInput {
    athlete: Athlete { body_mass_kg: 70.0, ftp_w: Some(250.0) },
    sessions: &sessions,
    next_session_start_min: Some(24 * 60 + 9 * 60), // again tomorrow at 09:00
    schedule: MealSchedule::default(),
})?;
// plan.carbs_g_per_kg, plan.protein_g, plan.entries (time, kind, carbs, protein)
```

## Rules

| Rule | Value | Source |
|---|---|---|
| Daily carbohydrate | light 3–5, moderate (~1 h) 5–7, high (1–3 h) 6–10, very high (4–5+ h) 8–12 g/kg | Thomas, Erdman & Burke 2016 (ACSM/AND/DC); Burke et al. 2011 (IOC) |
| Point within the range | piecewise linear in effective load (kJ/kg × intensity weight), anchors on the band boundaries | this crate; judgment call documented in `daily` |
| Daily protein | 1.2–2.0 g/kg, rising with load; ~0.3 g/kg per meal | Thomas et al. 2016; Moore et al. 2015; Areta et al. 2013 |
| Pre-ride | 1–4 g/kg, 1–4 h before (1 g/kg per hour of lead time, limited by wake time) | Thomas et al. 2016; Burke et al. 2011 |
| During | < 45 min: none · 45–75 min: 0–30 g/h or a mouth rinse · 75 min–2 h: 30–60 g/h · 2–2.5 h: 45–60 g/h · > 2.5 h: 60–90 g/h, glucose + fructose above 60 g/h; intensity picks the point | Jeukendrup 2014; Carter et al. 2004 |
| Recovery | next session < 24 h away: 1.0–1.2 g/kg/h for up to 4 h, within the day's budget; ~0.3 g/kg protein | Burke et al. 2011; Moore et al. 2009 |

Intensity picks the point within each range, and so does duration through
the work done. The [daily-load index](src/daily.rs) weights work per kg by
`IF / 0.75`, because carbohydrate's share of the energy rises with
intensity (Romijn et al. 1993; van Loon et al. 2001).

## Guarantees

- **Monotone:** more duration, intensity or work never lowers the daily
  targets, the pre-ride meal, the in-ride rate or the recovery rate. This
  is a property test.
- **Consistent:** the timeline is sorted and protein meets its target.
  Carbohydrate is exactly on target unless the session feeds alone exceed
  it, which the plan flags. In-ride feeds fall strictly inside the session.
- **Targets stay in their bands:** every daily target lies inside its
  load band's consensus range (unit test across loads).
- `#![no_std]` + `alloc`, no dependencies, panic-free under the same lints
  as the other zerofit crates.

## License

MIT OR Apache-2.0.
