; nightjar.mm — it sleeps because it is dark.
;
; `ancestor.mm` plus a photosensor and one gene. The gene shuts the chloroplast when the light
; falls below half of full daylight and opens it again at dawn, so the cell stops paying the
; working rate for machinery that cannot work.
;
; ---------------------------------------------------------------- why this could not be written
;                                                                   until ISA 15
;
; A photosensor has reported ambient light since M1 and no genome has ever read it. That was not
; an oversight. `read_sensor` divided the reading by `Q10_ONE`, and the light field is a `Q10`
; *fraction* of full daylight, so every regime in `scenarios/` — which runs between 96 and 1536 —
; came out as the integer **0**. Over a full cycle of `the_short_night` the field took twenty
; distinct values and the genome saw one of them, and it was zero the whole way round: the
; triangle peaks at 1023, one short of the divisor, so the reading never even reached 1.
;
; ISA 15 put the three ambient readings on `SENSE_GAIN`, the scale the *gradient* readings have
; used since M3 for exactly the same reason. The same cycle now reads:
;
;   54  76  98 121 143 166 188 210 233 255 233 211 189 166 144 121  99  77  54  32
;
; Noon is 255 and the darkest part of the night is 32, which is a range a genome can write a
; threshold in with a single `IMM`. `DUSK` below is 128 — half daylight.
;
; ---------------------------------------------------------------- and it loses, everywhere
;
; **Kept as a measured negative**, the way `hoarder.mm` is, because the law it taught is worth
; more than the genome. On `the_short_night`, twenty thousand ticks against `ancestor.mm`:
;
;   eye param   DUSK   population        against ancestor ~980
;         40     128   extinct by 3,000
;          0      32   980 966 958       loses
;          0      64   987 985 977       a tie, and the best it ever managed
;          0     128   878 834 901       loses
;
; Then, holding the genome and darkening the world — the obvious next suspect, since this world's
; "night" floor is 128, an eighth of full daylight:
;
;   night floor   ancestor   nightjar
;           128    992 978    987 985
;            64   1023 992    938 935
;            16   1025 983    931 894
;             0   1023 1001   905 877
;
; **A darker night makes it worse.** That is the result that settles it, because it is the
; opposite of what "the night is not dark enough" predicts. A bigger chloroplast — 153 `Q10` a
; tick to save instead of 42 — does not rescue it either.
;
; ---------------------------------------------------------------- the law it taught
;
; **A throttle gate pays when its input is absent and loses when its input is merely low.**
;
; `sleeper.mm` gates on carbon dioxide, and carbon dioxide is either in the cell or it is not.
; When it is not, the chloroplast can fix nothing, so shutting it costs exactly zero income and
; the saved upkeep is clear profit — it wins 5 of 5 seeds in `the_lean_water`.
;
; Light is a *continuum*. Below any threshold a genome can name, the chloroplast is still working,
; just less. So this gate always throws away real fixation to buy its upkeep back, and the darker
; the night the more of the cycle it spends in the low-but-not-zero shoulder where that trade is
; worst. There is no threshold that fixes it, because the only light level at which shutting is
; free is exactly zero, and a triangle is at zero for one tick.
;
; The corollary is a design rule for anything else that gets a dormancy gate: **gate on a
; quantity that goes to zero, not on one that fades.**
;
; ---------------------------------------------------------------- and the sensor has to be cheap
;
; The first draft carried a param-40 photosensor and went extinct by tick 3,000. The arithmetic
; says why before any run does:
;
;   photosensor param 40    28 Q10 a tick, paid always
;   chloroplast param 60    42 Q10 a tick, saveable and only while dark
;
; The eye cost more than half of the most the decision could ever return, and it was paid at noon
; as well as at midnight. **A sensor must cost less than the decision it informs**, and this is
; the first genome in the library where that is the binding constraint rather than a truism.
;
; `read_sensor`'s ambient-light path never reads `param`, so a photosensor's size buys nothing at
; all for this reading — it is pure cost. Anything watching only ambient light should build one at
; param 0.
;
; ---------------------------------------------------------------- what it does not do
;
; It does not bank against the night, and it must not try. `leak_cost` charges `Q10_ONE/64` on
; everything held above `energy_reserve`, which at 4,000 energy is thirty-one a tick against a
; whole-cell bill of under half a unit — `docs/ECONOMY.md` §17.1. A cell cannot save its way
; through a dark season, only spend less getting there.
;
; It also does not shut the mitochondrion. Respiration is the only income in the engine and the
; night is when a photo-autotroph lives on what it fixed by day, so the engine that burns it is
; the last thing to turn off. `sleeper.mm`'s header records what happened to a genome that tried.
;
; ---------------------------------------------------------------- the body
;
;   0  membrane
;   1  nucleus       48, because this genome is longer than the ancestor's 320-byte capacity
;   2  mitochondrion
;   3  chloroplast   the thing being switched
;   4  photosensor   the thing doing the switching

        EXPRESS #build
        EXPRESS #night
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     48              ; nucleus
        IMM     1
        IMM     1
        BUILD
        IMM     60
        IMM     3               ; chloroplast
        IMM     3
        BUILD
        IMM     50
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
        IMM     0               ; photosensor — param buys nothing here, so pay nothing
        IMM     8
        IMM     4
        BUILD
        RET

; ---------------------------------------------------------------- sleep through the dark
;
; `OGET ( idx slot -- v )`, reading 0 on slot 4: ambient light where the cell is standing.
;
; The comparison is the ancestor's own idiom from `#divide` — `CMP` then `ONE ADD` turns "less
; than" into a zero for `JMPZ` to catch. Above `DUSK` the chloroplast runs; below it, the cell
; stops paying three quarters of its upkeep for an organelle with no light to work on.
;
; There is no hysteresis here and none is needed: light crosses `DUSK` twice per two-thousand-tick
; period and never chatters across it. That was the property this genome was written to exploit,
; and it turned out to be the wrong property to want — smoothness is exactly what makes a light
; gate unprofitable, because a smooth signal is never *absent*. See the header.

        GENE    #night
        ZERO                    ; reading 0 — ambient light
        IMM     4               ; slot 4, the photosensor
        OGET
        IMM     64              ; DUSK — the best threshold measured, and still not a win
        CMP                     ; -1 if darker, 0 or 1 if not
        ONE
        ADD                     ; 0 if darker, non-zero if not
        JMPZ    dark
        IMM     255             ; daylight: run the chloroplast
        IMM     2
        SHL                     ; 1020, just under the Q10 clamp
        ZERO                    ; control 0 — the throttle
        IMM     3               ; the chloroplast
        OSET
        RET
dark:
        ZERO                    ; night: stop paying the working rate for it
        ZERO
        IMM     3
        OSET
        RET

; ---------------------------------------------------------------- feed

        GENE    #feed
        IMM     40
        IMM     11              ; carbon dioxide, the input to photosynthesis
        EAT
        DROP
        IMM     20
        IMM     14              ; and its oxidant
        EAT
        DROP
        IMM     16
        IMM     4               ; carbon, to build a body out of
        EAT
        DROP
        RET

; ---------------------------------------------------------------- keep house

        GENE    #grow
        IMM     255
        IMM     13              ; peroxide, out
        EMIT
        DROP
        IMM     8
        IMM     8               ; surplus sugar back to the water
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- divide

        GENE    #divide
        ONE
        ZERO
        OGET                    ; membrane slot 0, reading 1: energy
        IMM     100
        CMP
        ONE
        ADD
        JMPZ    lean
        GLEN
        SETLN
        GLEN
        BUD
        DROP
        ZERO
        SETPA
        ZERO
        SETPB
loop:
        COPYB
        LOOPLN  loop
        SPLIT
lean:
        RET
