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

    ; f_stat on the sample.txt file
    ld hl, filinfo_buf
    ld de, filename
    ld a, $96
    rst.lil $8

    ; print filename
    ld hl, filinfo_buf + 22
    ld bc, 0
    xor a
    rst.lil $18

    ; newline
    ld a, 10
    rst.lil $10

    ; check filesize is correct
    ld hl, (filinfo_buf)
    ld de, 46
    or a
    sbc hl, de
    ld a, h
    and a, l
    jr nz, exit_fail

exit_success:
    ld hl, success_msg
    ld bc, 0
    xor a
    rst.lil $18
    jr exit

exit_fail:
    ld hl, failure_msg
    ld bc, 0
    xor a
    rst.lil $18

exit:
    pop iy
    pop ix
    ld hl, 0
    ret
success_msg:
    .db "hostfs test passed", 13, 10, 0
failure_msg:
    .db "hostfs test FAILED", 13, 10, 0
filename:
    .db "regression_suite/sample.txt", 0
file:
    .ds 1
buffer:
    .ds 256
filinfo_buf:
    .ds 278 ; fatfs sizeof(FILINFO)
