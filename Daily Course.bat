@echo off
rem Today's course: the same city, era and deliveries for everyone today.
cd /d "%~dp0"
cargo run --release -p kwc-app -- --daily
