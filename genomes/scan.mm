; scan.mm — templates, base-paired jumps and conditional flow (SPEC §4.3).
;
; Complementary matching is the single most important mechanic for evolvability. A jump does
; not name an address; it emits a pattern and searches outward for the pattern that base-pairs
; with it. Genomes are therefore position-independent, so duplication, translocation and
; insertion produce working variants rather than rubble — and a damaged template finds a
; slightly wrong target instead of crashing.
;
; This genome counts down from 5, calls a subroutine each time round, and stops. It also
; carries a raw %pattern jump and a degenerate encoding, so the assembler's escape hatches
; are covered by the round-trip test.

        IMM     5
        ZERO
        RSTORE                  ; register 0 = counter

top:
        ZERO
        RLOAD
        JMPZ    done            ; forward search, conditional
        CALL    work
        ZERO
        RLOAD
        ONE
        SUB
        ZERO
        RSTORE
        JMPB    top             ; backward search
        HALT                    ; unreachable, and it stops the jump's four template letters
                                ; from fusing with the label's four below — a template is the
                                ; *maximal* run of NOP letters, so adjacent ones become one
done:
        ZERO
        SKIPZ                   ; a zero skips the next instruction and its template
        JMPF    %1010           ; skipped, and matches nothing anyway
        HALT
        JMPB~2  top             ; a non-canonical encoding of JMPB
        HALT                    ; separator again

work:
        ONE
        ONE
        ADD~1                   ; and of ADD
        DROP
        RET
