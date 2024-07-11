#!/bin/bash
echo "Agon Light CPU Emulator regression test suite running..."
rm -f agon_regression_suite/regression_test.out \
   sdcard/regression_suite/animal2.bas \
   sdcard/regression_suite/animal2.bbc \
   sdcard/regression_suite/helloworld.bin \
   sdcard/regression_suite/helloworld16.bin

gunzip -c sdcard_regression_suite.img.gz > sdcard_regression_suite.img

cargo run --release -- --sdcard-img sdcard_regression_suite.img --unlimited-cpu < agon_regression_suite/regression_test_script.txt | tee agon_regression_suite/regression_test.spi_sd.out
cargo run --release -- --unlimited-cpu < agon_regression_suite/regression_test_script.txt | tee agon_regression_suite/regression_test.hostfs.out
echo

if ! cmp agon_regression_suite/regression_test.spi_sd.out agon_regression_suite/regression_test.expected; then
	echo "[SPI SDcard image] Regression suite output differs from expected in run: SPI SDcard image"
	diff -u agon_regression_suite/regression_test.expected agon_regression_suite/regression_test.spi_sd.out
fi

if ! cmp agon_regression_suite/regression_test.spi_sd.out agon_regression_suite/regression_test.expected; then
	echo "[HostFS] Regression suite output differs from expected in run: SPI SDcard image"
	diff -u agon_regression_suite/regression_test.expected agon_regression_suite/regression_test.spi_sd.out
fi

if cmp agon_regression_suite/regression_test.spi_sd.out agon_regression_suite/regression_test.expected; then
	printf '\x1b[32m[SPI SDcard image] Test suite passed\x1b[0m\n'
else
	printf '\x1b[31m[SPI SDcard image] Test suite failed\x1b[0m\n'
fi

if cmp agon_regression_suite/regression_test.hostfs.out agon_regression_suite/regression_test.expected; then
	printf '\x1b[32m[HostFS] Test suite passed\x1b[0m\n'
else
	printf '\x1b[31m[HostFS] Test suite failed\x1b[0m\n'
fi
