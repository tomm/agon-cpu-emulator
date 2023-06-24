#!/bin/bash
echo "Agon Light CPU Emulator regression test suite running..."
rm -f agon_regression_suite/regression_test.out \
   sdcard/regression_suite/animal2.bas \
   sdcard/regression_suite/animal2.bbc \
   sdcard/regression_suite/helloworld.bin \
   sdcard/regression_suite/helloworld16.bin
cargo run --release -- --unlimited-cpu < agon_regression_suite/regression_test_script.txt | tee agon_regression_suite/regression_test.out
echo

if cmp agon_regression_suite/regression_test.out agon_regression_suite/regression_test.expected; then
	printf '\x1b[32mTest suite passed\x1b[0m\n'
	exit 0
else
	echo "Regression suite output differs from expected:"
	diff -u agon_regression_suite/regression_test.expected agon_regression_suite/regression_test.out
	printf '\x1b[31mTest suite failed\x1b[0m\n'
	exit 1
fi
