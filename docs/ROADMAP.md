# Roadmap: demo polish

The live demo at <https://c4w1.github.io/zerofit/> is linked from the
resume, so this round makes what a visitor sees correct and polished.
Items are ticked as they land on `main`. The adaptive plan engine comes
later.

## Fueling

- [ ] 1. Look-ahead fueling. The demo's Friday is 3.0 g/kg before Saturday's
  4-hour ride. A day's carb target must account for the next day's planned
  load ("fuel for the work required", Impey et al. 2018): raise it the day
  before any session over 90 min or any key high-intensity session, and
  apply ACSM 2016 carb-loading guidance before very long or A-priority
  days. Show the reason on the day card ("Raised: 4 h ride tomorrow").
  Golden-day tests (rest before a long ride, rest before rest, day before
  an A race) and a proptest that a day's target never drops when
  tomorrow's load rises.
- [ ] 2. Load-band labels consistent with the g/kg numbers, with a visible reason
  (currently Tuesday 6.4 g/kg "Moderate" vs Thursday 6.5 g/kg "High").

## Demo data

- [ ] 3. My FTP is real: 326 W held for 65 minutes on a trainer. The ride is at
  tests/fixtures/ftp-65min.fit with intervals.icu values in
  intervals_icu.json. Strip serials, validate it, add it to the demo, and
  set the demo athlete's FTP to 326 W. Report the resulting eFTP and CP
  fits and explain any mismatch with 326 W.
- [ ] 4. Add 12-16 weeks of realistic synthetic training history for this
  athlete (labeled synthetic in the UI), including some maximal efforts so
  the CP fit is credible, so the fitness chart doesn't sit at CTL 8.
- [ ] 5. Precompute the demo analysis at build time so first paint shows real
  numbers instead of "..." placeholders.

## UI

- [ ] 6. The top nav overflows at phone width (horizontal scrollbar, "Fueling"
  cut off). Use a layout that fits, e.g. a bottom tab bar on narrow
  screens.
- [ ] 7. The fueling timeline's hairline bars are hard to read. Use wider bars or
  a cumulative-carbs line against the daily target, with sessions shaded.
- [ ] 8. Fueling day tabs have no accessible names; label each (day, date, g/kg,
  band).
- [ ] 9. Fix the "232± 6 W" formatting on the fitness page.
- [ ] 10. Take Playwright screenshots of every page at 390 px and 1280 px wide,
  light and dark. Fix anything broken, cramped, or empty on first load.

## Wrap-up

- [ ] 11. Re-run Lighthouse and axe on all pages and report the scores.
- [ ] 12. Refresh the numbers in docs/RESUME.md and the READMEs (test counts,
  Lighthouse, eFTP/CP) using only values measured in this repo, and list
  which resume numbers changed.
- [ ] 13. Practice branch, LOCAL ONLY (do not push): create a branch `practice`
  where the bodies of these functions are replaced with todo!() and their
  tests are kept: FIT file header parsing, record header parsing,
  definition message parsing, CRC-16, normalized power, the CTL/ATL
  daily update, and W' balance. Add docs/PRACTICE.md listing them in a
  sensible order, each with the spec section or formula it relies on, a
  hint, and the command to run its tests.
