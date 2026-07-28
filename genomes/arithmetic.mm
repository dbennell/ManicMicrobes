; arithmetic.mm — saturating arithmetic, the stack, registers and scratch RAM.
;
; Exists to be a genome that touches most of the 0x00-0x1F range, so that assembler,
; disassembler and VM are exercised over the arithmetic block rather than only over control
; flow. It computes nothing useful on purpose: there is no fitness function anywhere in this
; project, and a genome in `genomes/` is a test fixture, not a target.
;
; The saturation is the point. 32767 + 1 stays 32767, so a one-bit mutation cannot flip a
; cell from "very fast forward" to "very fast reverse" (SPEC §3).

        IMM     255:8
        IMM     127:8
        MUL                     ; 32385
        DUP
        ADD                     ; would be 64770; saturates to 32767
        DUP
        ONE
        ADD                     ; still 32767
        ZERO
        RSTORE                  ; register 0 = the saturated value

        ZERO
        RLOAD
        NEG
        ABS
        IMM     3
        DIV
        IMM     7
        MOD
        IMM     5
        MIN
        IMM     2
        MAX

        ONE
        SHL
        IMM     3
        SHR
        IMM     0xF0
        AND
        IMM     0x0F
        OR
        IMM     0x33
        XOR
        NOT

        DUP
        IMM     4
        STORE                   ; scratch RAM word 4
        IMM     4
        LOAD
        CMP                     ; ( a b -- sign(a-b) ), so this is 0
        DROP

        ZERO
        DIV                     ; division by zero yields zero, never a fault
        DROP
        HALT
