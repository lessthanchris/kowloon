@echo off
cd /d "%~dp0"
cargo run --release -p kwc-app
if errorlevel 1 pause
