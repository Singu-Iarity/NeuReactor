# Physics notes (PWR model, v0.1)

| Phenomenon | Model | Verified by |
|---|---|---|
| Neutron kinetics | Point kinetics, 6 delayed groups (Keepin U-235), Λ = 20 µs, backward Euler | prompt jump, inhour stable period (≤3 %) |
| Xe-135 / I-135, Sm-149 / Pm-149 | Exact-exponential per-step updates, φ₀ = 3×10¹³ n/cm²s, eq. worth −2800 / −700 pcm | equilibrium, post-trip peak 7–12 h |
| Decay heat | 9 exponential groups fitted to Way–Wigner 0.0622 t^-0.2 | within 15 % from 1 s to 1 day |
| Rods | 6 banks, 228 steps, S-curve integral worth, 72 steps/min, 2.2 s scram | unit tests |
| Feedback | Doppler −3 pcm/K (fuel), moderator −20 pcm/K, boron −10 pcm/ppm | power defect ≈ 1350 pcm |
| Fuel | Lumped heat capacity 3.2×10⁷ J/K, ΔT(fuel–coolant) 320 K at full power | |
| Primary loop | 4 mixing volumes (core, hot leg, SG, cold leg), cp 5.5 kJ/kgK, 17 700 kg/s | steady state, load follow |
| Pressurizer | Inventory → level from loop density; dp/dt from insurge compression + heater/spray/loss energy; PORV 16.2 MPa, safeties 17.2 MPa | scram transient |
| SG / secondary | Lumped saturated SG (Tsat state), UA from design point with flow and dryout degradation, choked valve flow | scram, load follow |
| Properties | Saturation steam-table interpolation (IF97 reference points), compressed liquid density at 15.5 MPa | unit tests |

## Fidelity roadmap
1. 1-D axial nodal two-group diffusion with per-node feedback and xenon (axial offset, xenon oscillations, rod shadowing), then radial channels.
2. Per-node fuel pin radial conduction, DNBR/CHF margin, clad temperature.
3. Full IAPWS-IF97, two-region non-equilibrium pressurizer, SG shrink/swell, drift-flux two-phase (needed for BWR/RBMK).
4. ECCS, auxiliary feedwater, diesel generators, station blackout.
5. ANS-5.1 decay heat with irradiation history; burnup and plutonium build-up.
