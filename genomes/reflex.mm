; reflex.mm — a nerve net, made of nothing the engine calls a nerve.
;
; SPEC §8.1 says a soft junction is "the conjugation channel, the synapse and the infection
; route, all the same mechanism". `parasite.mm` is the infection route. This is the synapse,
; and it exists to find out what a nervous system actually costs here rather than to win
; anything — like `parasite.mm`, it is a demonstration and not a competitive genome.
;
; ---------------------------------------------------------------- what it is
;
; A **nerve net**, deliberately, not a brain. Every cell does the same three things:
;
;   take up transmitter from the water   (excitation)
;   push some to whatever it is joined to (transmission)
;   beat its cilium in proportion to what it holds (response)
;
; So a stimulus touching one cell produces motion in cells it never touched, and no cell has a
; role. That is the primitive case — a hydra, not a flatworm — and it is the right first target
; because it needs no differentiation. **Differentiation is M7 acceptance 6's problem and is
; deliberately not tested here**; conflating the two is how you get a result that cannot say
; which half failed.
;
; ---------------------------------------------------------------- the transmitter is matter
;
; The part with no counterpart in real neurophysiology, and the most interesting thing this
; genome found.
;
; Matter is exactly conserved (I4), so a cell **cannot invent a signal**. Every unit of
; transmitter this net passes around was eaten out of the water and will be excreted back into
; it. A synapse here costs matter as well as energy, and the total signal a network can hold is
; bounded by how much signal chemical the environment contains.
;
; A real neuron escapes this by re-pumping an ion gradient it never spends. The organelle for
; that is the `PUMP`, catalogue slot 5 — which is built, priced, and **read by no mechanism
; anywhere**. So the escape hatch is unavailable, and a nerve net in this engine is a bucket
; brigade. That is a finding about the engine, not a fault in the genome.
;
; ---------------------------------------------------------------- three warts, in place
;
; 1. **`JXFER 0` means energy, not chemical 0.** `resolve_transfer` reads `what == 0` as the
;    energy channel, so `signal_a` is unreachable at its own index and has to be addressed as
;    16, which `chem_index` wraps back to 0. Every other opcode in the ISA takes a chemical at
;    its face value. It is written as `IMM 16` below with this comment beside it, because the
;    obvious `ZERO` silently donates energy to your neighbour instead.
;
; 2. **A genome cannot see its own junctions.** `JOIN` returns 1 optimistically and its own
;    comment says "the genome finds out by looking" — but the junction port, catalogue slot 10,
;    has no `OGET` readings at all and is not even required in order to `JOIN`. It is a second
;    paid-for no-op beside the pump. So `#wire` below joins blindly every tick and `#relay`
;    transmits blindly every tick, because neither can ask. `parasite.mm`'s `#infect` has the
;    same problem and is worse off for it: its `JMPNZ connected` tests a reading that is always
;    zero, so that state machine can never reach its second state.
;
; 3. **Nothing decays inside a cell.** Interior decay was tried and reverted for a good reason
;    (`metabolism.rs:649` — peroxide decaying into carbon dioxide inside the cytoplasm made
;    hoarding waste an advantage). The consequence here is that arriving transmitter accumulates
;    forever and the net latches on and never releases. So `#grow` implements the leak **in
;    instructions**, at three of them per tick, which is what a per-chemical interior decay rate
;    would do for nothing.
;
; ---------------------------------------------------------------- what it measured
;
; `reflex_probe` is the run; this is the summary. Three cells, hard-junctioned in a line, a
; stimulus of transmitter on the head cell's square only.
;
;   * **It works, and against the shipped chemical table it is worth almost nothing.** Every cell
;     reaches full cilium power within about thirty ticks whether the wire exists or not, because
;     `signal_a` diffuses at `Q10/4` — the fastest rate in the table — and three cells two squares
;     apart are one puddle on the timescale a genome runs at. Force its diffusion to zero and the
;     wire becomes the whole story: the wired chain still conducts, the control propagates
;     nothing. **A nervous system needs a transmitter that does not diffuse**, and there is not
;     one in the default table.
;
;   * **The middle cell conducts without ever being excited.** It reads zero transmitter and zero
;     thrust throughout while the cell behind it climbs to full: resolve applies every intent in
;     one pass in slot order, so a chain in ascending slot order propagates end to end in a
;     single tick and the same chain in descending order would take one tick a hop. Conduction
;     velocity is a function of birth order.
;
;   * **A slow transmitter is what makes it worth anything.** Sweeping `signal_a`'s diffusion
;     down by halves, the tail cell reaches half thrust after 29 ticks on the wire at *every*
;     rate from `Q10/8` to zero, while diffusion takes 37, 53, 101, 165, 213, 277, 341 and then
;     never. Conduction time is independent of the chemistry and diffusion time is not, which is
;     the entire case for having a nervous system. A transmitter as slow as detritus — already in
;     the table — is worth 72 ticks.
;
;   * **It can assemble itself, and the result does not conduct.** With hard junctions the genome
;     builds a connected chain unaided. But `resolve_join` hands each end its lowest free slot, so
;     the inbound link lands in slot 0 — the first slot `#relay` transmits into — and every cell
;     sends the signal back the way it came. A genome cannot choose a slot, read which slot a
;     junction landed in, or tell inbound from outbound, so it cannot build a directed arc. That
;     is the one thing here that is actually blocking, and the fix is `OGET` readings on the
;     junction port, which is already built and priced and answers zero to everything.
;
;   * **A directed arc costs two junction slots per cell.** Transmitting into slot 0 at both ends
;     makes the middle cell push back towards the head first, and the signal sloshes backwards
;     while the tail gets nothing — measured, before the probe was corrected. Downstream has to
;     sit in a slot the genome writes to and upstream in one it does not. At four slots a cell,
;     a middle cell is half full holding a chain of three together.
;
; ---------------------------------------------------------------- what it does not do
;
; **It does not divide.** A daughter is not wired to anything and would break the chain the
; measurement is about, so the replication loop is omitted rather than guarded. It is therefore
; not in `m8_ecology::ORGANISMS` and is not meant to be.
;
; Chemical indices, default table (SPEC §7.1):
;   0  signal_a          the transmitter. The engine ascribes it no meaning, which is the point
;   4  carbon           structural
;   8  sugar            the energy substrate
;   11 carbon_dioxide   the waste photosynthesis eats
;   13 peroxide         respiration's poison
;   14 oxygen           the oxidant
;
; Organelle slots:
;   0  membrane      (always)
;   1  nucleus
;   2  mitochondrion
;   3  chloroplast   — a nervous system has to pay for itself
;   4  cilium        — the output. Mount angle defaults to +x in every clone, so a net that
;                      fires together swims together, which is what makes the response legible
;   5  touch sensor  — the only way to get a handle to `JOIN`

        EXPRESS #build
        EXPRESS #feed
        EXPRESS #wire
        EXPRESS #relay
        EXPRESS #grow
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     48              ; nucleus: 384 bytes, room for 334 and some drift
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
        IMM     40
        IMM     6               ; cilium
        IMM     4
        BUILD
        IMM     12
        IMM     9               ; touch sensor
        IMM     5
        BUILD
        RET

; ---------------------------------------------------------------- feed
;
; The ancestor's, unchanged, plus the transmitter. `EAT` is clamped to what is in the square and
; to what the cell can hold, so asking for more than there is costs one instruction and nothing
; else — which is also the whole of this net's receptor: a cell is excited by standing in
; transmitter.

        GENE    #feed
        IMM     40
        IMM     11              ; carbon dioxide, the input to photosynthesis
        EAT
        DROP
        IMM     4
        IMM     4               ; carbon, and only a little: see #grow on why
        EAT
        DROP
        IMM     8
        ZERO                    ; signal_a — the stimulus, taken up as matter
        EAT
        DROP
        RET

; ---------------------------------------------------------------- wire
;
; Clones share a receptor key, so joining a sibling is nearly free (§8.2). But a cell cannot
; *read* its own key — that is the whole basis of the mechanic — so a lineage that wants to
; recognise itself has to **set** the key to a constant it also carries as an immediate. Both
; ends of that are one byte in the genome, so a mutation to either drops a cell out of its own
; colony, which is the Red Queen cost §8.2 describes, paid here in the smallest possible coin.

        GENE    #wire
        IMM     42
        SETKEY                  ; a shared secret a clone can set and never read
        ZERO
        IMM     5
        OGET                    ; touch sensor 0: how many cells am I against
        JMPZ    alone
        IMM     42              ; the same constant every clone sets, so the join is cheap
        ONE                     ; kind 1 — HARD, and that is measured rather than chosen.
                                ; A soft junction breaks past `soft_max_range`, an absolute three
                                ; squares, and two tangent cells of the size this genome settles
                                ; at are exactly three squares apart before any drift. The channel
                                ; SPEC §8.1 calls the synapse cannot span two grown cells at all.
                                ; `resolve_transfer` never checks the kind, so a strut carries
                                ; signal — which is the only reason a nerve net is possible here.
        ONE
        IMM     5
        OGET                    ; touch sensor 1: a handle for the nearest
        JOIN
        DROP                    ; optimistic, and there is nothing to check it against
alone:
        RET

; ---------------------------------------------------------------- relay
;
; The whole nervous system, and it is nine instructions.
;
; Note what is *absent*: there is no summation code. Several presynaptic cells pushing into the
; same chemical arrive in one interior pool, so the cytoplasm is the dendritic integrator and
; the weighted sum is free. The weight is how much each sender chooses to push — `IMM 4` here,
; one genome byte, so a single copy error retunes a synapse without breaking the program.
;
; Junction slots 0 and 1, not all four: a chain reaches both its neighbours that way, and each
; extra slot is five more instructions out of sixteen a tick.

        GENE    #relay
        IMM     4               ; the synaptic weight, in whole units of transmitter
        IMM     16              ; chemical 0. NOT `ZERO`, which JXFER reads as the energy channel
        ZERO                    ; junction slot 0
        JXFER
        DROP
        IMM     4
        IMM     16
        ONE                     ; junction slot 1
        JXFER
        DROP
        RET

; ---------------------------------------------------------------- respond, and keep house
;
; The diet is leaner than the ancestor's and that is not a style choice. This genome does not
; divide, so it cannot shed solute the way every other shipped genome does, and on the ancestor's
; diet it accumulated to seventeen interior capacities against a turgor threshold of four and died
; of the quadratic charge at tick 1,800. Eating a quarter as much carbon and excreting the oxidant
; — which photosynthesis *makes*, and which the ancestor eats anyway — holds it at one capacity
; and 2,107 energy indefinitely. `reflex_probe::an_undivided_cell_dies_of_turgor` is the run.
;
; The cilium is driven straight off how much transmitter the cell is holding: a membrane reading
; of 5 is internal chemical 0, and `control[0]` on a cilium is signed power clamped to ±1024. At
; a gain of 64, sixteen units of transmitter is full thrust. That is the entire motor pathway.

        GENE    #grow
        IMM     5
        ZERO
        OGET                    ; membrane reading 5: my own signal_a
        IMM     64
        MUL                     ; gain
        ZERO                    ; control index 0 — power
        IMM     4
        OSET                    ; cilium, slot 4
        IMM     2
        ZERO
        EMIT                    ; the leak, done in instructions because the chemistry has none
        DROP
        IMM     255
        IMM     13              ; peroxide, out, or the cell ages and dies
        EMIT
        DROP
        IMM     8
        IMM     8               ; surplus sugar back to the water
        EMIT
        DROP
        IMM     255
        IMM     14              ; and the oxidant, which photosynthesis *makes*
        EMIT
        DROP
        RET
