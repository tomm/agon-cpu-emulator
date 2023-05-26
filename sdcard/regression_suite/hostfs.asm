    .assume adl=1
    .org $40000
    jp start
    .align $40

    .db "MOS"
    .db 0 ; version
    .db 1 ; ADL enabled
start:
    push ix
    push iy

    ; f_open
    ld hl, filename
    ld c, 1
    ld a, $a
    rst.lil $8
    ld (file), a

    ; f_lseek
    ld (file), a
    ld c, a
    ld hl, 4 ; offset in file
    ld e, 0
    ld a, $1c
    rst.lil $8

    ; f_read
    ld a, (file)
    ld c, a
    ld hl, buffer
    ld de, 11
    ld a, $1a
    rst.lil $8
    ; de should be bytes read

    ; f_close
    ld a, (file)
    ld c, a
    ld a, $b
    rst.lil $8

    ; print read data to terminal
    ld hl, buffer
    ld bc, 11
    xor a
    rst.lil $18

    ; newline
    ld a, 10
    rst.lil $10

    pop iy
    pop ix
    ld hl, 0
    ret
filename:
    .db "regression_suite/sample.txt", 0
file:
    .ds 1
buffer:
    .ds 256
