    .assume adl=1
    .org $40000
    jp start
    .align $40

    .db "MOS"
    .db 0 ; version
    .db 1 ; ADL

	macro print str
		push hl
		push bc
		push af
		ld hl, str
		ld bc, 0
		xor a
		rst.lil $18
		pop af
		pop bc
		pop hl
	endmacro

start:
	push ix
	push iy

	xor a
	ld (irq_counter), a

	ld hl, 0
	ld a,($10c)
	ld l, a
	ld a,($10d)
	ld h, a

	; skip over CALL ($c3)
	inc hl
	; load address of jump into vector table 2 (in ram)
	ld hl,(hl)

	; write CALL timer_irq_handler to vector table 2
	ld a, $c3
	ld (hl), a
	inc hl
	ld de, timer_irq_handler
	ld (hl), de
	

	; timer reset count
	ld hl, $fff0
	out0 ($84), l
	out0 ($85), h
	; enable timer, with interrupt
	ld a, $43
	out0 ($83), a

	; read it back and display
	print msg_timerctl
	in0 a,($83)
	call dump_a
	call newline

	ld hl, 0
loop:
	inc hl
	in0 b,($84)
	in0 a,($85)
	or b
	jr nz, loop

	;print msg1
	;call dump_hl24
	;call newline
	print msg2
	ld a, (got_irq)
	call dump_a
	call newline

	; enable timer, with interrupt and CONTINUOUS mode
	ld a, $53
	out0 ($83), a

	; read it back and display
	print msg_timerctl
	in0 a, ($83)
	call dump_a
	call newline

loop2:
	; wait for 5 timer interrupts (actually 4 since we had one already)
	ld a, (irq_counter)
	cp 5
	jr nz, loop2

	print msg3
	ld a, (irq_counter)
	call dump_a
	call newline

	print msg2
	ld a, (got_irq)
	call dump_a
	call newline

	; disable timer
	xor a
	out0 ($83), a

	ld hl,0
	pop iy
	pop ix
	ret

msg1:
	.db "Loop iterations waiting for timer: ",0
msg2:
	.db "Value set from timer interrupt: ", 0
msg3:
	.db "IRQ counter: ", 0
msg_timerctl:
	.db "Timer CTL read back: ", 0

got_irq:
	.db 0
irq_counter:
	.db 0

timer_irq_handler:
	di
	push af
	in0 a,($83)
	ld (got_irq),a
	ld a,(irq_counter)
	inc a
	ld (irq_counter), a
	pop af
	ei
	reti.l

dump_hl24:
	push hl
	inc sp
	pop af
	dec sp
	call dump_a
	ld a,h
	call dump_a
	ld a,l
	call dump_a

	ret

newline:
    push af
    ld a,13
    rst.lil $10
    ld a,10
    rst.lil $10
    pop af
    ret

dump_a:
    push af
    push bc
    push de
    push hl
    ld b, a

    ; output high nibble as ascii hex
    srl a
    srl a
    srl a
    srl a
    ld de,hex_chr
    ld hl,0
    ld l,a
    add hl,de
    ld a,(hl)
    rst.lil $10

    ; output low nibble as ascii hex
    ld a, b
    and $f
    ld de,hex_chr
    ld hl,0
    ld l,a
    add hl,de
    ld a,(hl)
    rst.lil $10

    pop hl
    pop de
    pop bc
    pop af
    ret

hex_chr:
    .db "0123456789ABCDEF"

