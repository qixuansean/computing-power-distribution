@echo off
cd C:\Users\qixua\Desktop\OCN
cargo check -p ocn-core 2>check.out
echo Check complete. Exit code: %errorlevel%
type check.out