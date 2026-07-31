@echo off
taskkill /F /IM cockroach_overlay.exe >nul 2>nul
taskkill /F /IM cockroach_swarm_20.exe >nul 2>nul
start "" "%~dp0cockroach_overlay.exe" --seed 20260731
