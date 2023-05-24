# Agon CPU Emulator

An emulator of the Agon Light CPU (eZ80) and CPU-controlled peripherals.
A complete, graphical emulator involves an emulation of the Agon VDP.

For full graphical Agon Light emulation, see [Agon Light Emulator](https://github.com/astralaster/agon-light-emulator),
which uses *Agon CPU Emulator* to emulate the CPU-side of the system.

This Agon CPU Emulator can be used stand-alone, as a terminal-mode emulation
of Agon Light:

```
cargo run --release
```

This will pass stdin input to the Agon CPU as VDP keypress packets.
You can use this for a scripted agon emulation:

```
$ echo "credits
cat" | cargo run -r
Tom's Fake VDP Version 1.03
Agon Quark MOS Version 1.03

*credits
FabGL 1.0.8 (c) 2019-2022 by Fabrizio Di Vittorio
FatFS R0.14b (c) 2021 ChaN

*cat
Volume: hostfs

1980/00/0000:00 D     4096 agon_regression_suite
1980/00/0000:00       1518 LICENSE
1980/00/0000:00 D     4096 src
1980/00/0000:00 D     4096 target
1980/00/0000:00 D     4096 basic
1980/00/0000:00        234 Cargo.lock
```

This is how the regression suite is implemented. You can run the
regression suite with:

```
sh ./run_regression_test.sh
```
