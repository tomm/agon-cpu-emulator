    .assume adl=1
    .org $40000
    jp start
    .align $40

    .db "MOS"
    .db 0 ; version
    .db 1 ; ADL enabled
start:
    ld hl, hello
    ld bc, 0
    xor a
    rst.lil $18
    ld hl, 0
    ret
hello:
    .db "Hello world ADL!", 13, 10, 0
