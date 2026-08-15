; sentinel.mm — a predator that looks before it stabs.
;
; `predator.mm` is the same lineage with the other half missing, and shipping the pair is the
; point. It carries a spike and a stomach and holds the spike out permanently, and so it kills
; its own daughters: a spike damages what it touches and has no idea what it is touching, a
; newborn is born inside her mother's reach, and neither of them can move. The measurement is in
; that genome's header. It had to have its weapon turned down to a sixtyfourth to have
; descendants at all.
;
; This one carries a badge and a touch sensor, and every cycle it asks one question: **is the
; nearest thing wearing what I am wearing?** If it is, the spike goes in. If it is not, the spike
; goes out — all the way out, to the half extension that sterilised `predator.mm`, because a
; weapon you can put away is a weapon you can afford to make sharp.
;
; Nothing in the engine knows what a friend is. `TouchSensor` reading 3 reports that the thing in
; front of you is wearing 210 and says nothing whatever about what that means. This genome is the
; entire opinion that 210-like-me means do not stab. Another lineage is free to hold the opposite
; opinion, or to wear 210 without meaning it — a badge costs nothing to forge, which is what makes
; it worth having rather than an identity card issued by the physics.
;
; What it buys, one founder in `soup.ron` over 2400 ticks, against `predator.mm` — which had to
; have its weapon turned down to a sixtyfourth to breed at all:
;
;                  spike   population   energy
;   predator.mm       16           64        0
;   sentinel.mm      512           79      593
;
; Sharper by a factor of thirty-two, and it leaves more descendants, while carrying an extra
; organelle and a hundred and eleven more bytes. A weapon you can put away is a weapon you can
; afford to make sharp.
;
; Two things it does *not* do, and both are honest limits rather than oversights:
;
;   * It reads the *nearest* neighbour only. Standing between a daughter and a stranger it will
;     arm, and the spike damages everything in reach, so it will catch her too. Discrimination
;     here is a decision about the crowd taken from one sample of it.
;   * It compares badges rather than identity. A mimic that wears the same badge is safe from it
;     for free. That is the arms race and it is supposed to be available.

        EXPRESS #build
        EXPRESS #dress
        EXPRESS #arm
        EXPRESS #digest
        EXPRESS #watch
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     64              ; nucleus: 512 bytes
        IMM     1
        IMM     1
        BUILD
        IMM     55
        IMM     3               ; chloroplast
        IMM     3
        BUILD
        IMM     50
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
        RET

; ---------------------------------------------------------------- put the colours on
;
; The badge is inherited, so a daughter is already wearing this before she has run a single
; instruction — which is the only reason any of it works, since the ticks she is in danger are
; the ticks before her first expression cycle. Setting it here as well costs two instructions and
; means the genome and the marker cannot drift apart: a lineage that mutates this immediate
; changes what it wears *and* what it recognises, in the same stroke, and speciates.
;
; A *founder* is the exception, and it is worth knowing before placing two of them touching. She
; is seeded bare-faced and does not put her colours on until her first cycle has run, so for
; those few ticks she is a stranger to her own kind — two founders placed adjacent will kill each
; other before either is dressed. Spread them, as every seeding path already does.
;
; 210 fits in a byte and so costs one instruction. The badge is fifteen bits and a larger one has
; to be composed from several — worth paying for a lineage that expects to be mimicked, since a
; number a single byte mutation cannot reach is a number a forger cannot stumble onto, and not
; worth it here. This genome is already the longest in `genomes/` and length is the thing it can
; least afford: the copy loop is two instructions a byte, so every byte is another tick between
; meals.

        GENE    #dress
        IMM     210
        SETBADGE
        RET

; ---------------------------------------------------------------- the weapon and the eye
;
; Slot 5 the spike, slot 7 the touch sensor. The sensor is the whole difference from
; `predator.mm` and it is not free — it is another organelle drawing upkeep every tick, against
; a lineage that was already the most expensive thing in `genomes/`. What it buys is the right
; to carry a spike at full extension, which is worth more.

        GENE    #arm
        IMM     80
        IMM     12              ; spike
        IMM     5
        BUILD
        IMM     40
        IMM     9               ; touch sensor
        IMM     7
        BUILD
        RET

; ---------------------------------------------------------------- the stomach

        GENE    #digest
        IMM     70
        IMM     11              ; lysosome
        IMM     6
        BUILD
        IMM     255
        IMM     2
        SHL                     ; 1020, near full throttle
        ZERO                    ; control 0 — digestion rate
        IMM     6
        OSET
        RET

; ---------------------------------------------------------------- friend or foe
;
; The whole of it. Read what the nearest thing is wearing, read what I am wearing, and compare.
;
; Reading my own badge back rather than comparing against the immediate in `#dress` is
; deliberate: it means a mutation that moves the badge moves both halves at once and the cell
; still knows its own children. Hard-coding the number here would make every such mutation an
; instant matricide.
;
; `CMP` leaves 0 when they match, so `JMPZ` is "one of mine — put it away".
;
; The first test is the one that is easy to leave out and fatal to leave out. A cell touching
; nobody reads a badge of zero, which differs from its own — so without it a solitary cell arms
; permanently and is back to being `predator.mm`, paying the dearest upkeep in the catalogue to
; menace open water. Measured: the same genome with this line and without it goes from ninety-nine
; descendants to none.

; **It asks the touch sensor how many things are in reach, and it used to ask what the nearest one
; was wearing.** That looked equivalent and was not, and the difference is the whole of why this
; genome had never wounded anything in any run. "Nobody there" reads as a badge of zero — and so
; does *somebody wearing nothing*, which is every cell `World::place_founders` seeds and every
; genome in this library but two. So the guard against menacing open water was also a guard
; against attacking anything that had not chosen a badge, and the weapon was never drawn.
;
; Measured, eight of these among eight `ancestor.mm` over eight thousand ticks: from 0 cell-ticks
; with a spike out and 0 wounds, to a predator that draws on a stranger and puts the spike away
; for its own children. It is still far more selective than the kin-blind `predator.mm`, which is
; the point — that one is armed on 7,980 ticks of 8,000.
;
; Found by a dial sweep in which making the weapon eight times cheaper to hold changed the
; population by not one cell, which can only happen if the cost was never charged.
; `economy_probe::whether_the_armed_lineages_ever_draw` is the guard and `docs/ECONOMY.md` §4a is
; the account. Reading 0 is `TouchReading::contacts` and has been there since M3 — nothing needed
; building; the genome was asking the wrong question of a sensor that could already answer the
; right one.
        GENE    #watch
        ZERO
        IMM     7               ; touch sensor reading 0: how many are in reach
        OGET
        JMPZ    kin             ; nobody in reach, so nothing to point it at
        IMM     3
        IMM     7               ; reading 3: the nearest one's badge
        OGET
        IMM     22
        ZERO                    ; membrane reading 22: my own badge.
                                ; Was 21, and moved when the chemical table gained dinitrogen at
                                ; ISA 11: the membrane's scalars are laid out *after* the
                                ; chemicals, so widening the table shifts every reading past
                                ; them. That is what the version stamp is for, and it is the one
                                ; sharp edge of adding a chemical.
        OGET
        CMP
        JMPZ    kin
        IMM     128
        IMM     2
        SHL                     ; 512 — half extension, the setting that sterilised predator.mm
        ZERO                    ; control 0 — signed extension
        IMM     5
        OSET
        RET
kin:
        ZERO                    ; sheathed
        ZERO
        IMM     5
        OSET
        RET

; ---------------------------------------------------------------- feed

        GENE    #feed
        IMM     40
        IMM     11
        EAT
        DROP
        IMM     20
        IMM     14
        EAT
        DROP
        IMM     16
        IMM     4
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
        IMM     8
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- divide

        GENE    #divide
        ONE
        ZERO
        OGET
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
