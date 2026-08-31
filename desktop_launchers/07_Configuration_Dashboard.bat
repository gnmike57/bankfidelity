@echo off
title BankFidelity + UFO Master Configuration
echo Opening master configuration files...
if exist "C:\bankfidelity\bankfidelity\.env" (start notepad.exe "C:\bankfidelity\bankfidelity\.env") else (echo [WARN] .env not found at C:\bankfidelity\bankfidelity\)
if exist "C:\ufo\ufo\config\ufo\agents.yaml" (start notepad.exe "C:\ufo\ufo\config\ufo\agents.yaml") else (echo [WARN] agents.yaml not found)
if exist "C:\ufo\ufo\config\ufo\system.yaml" (start notepad.exe "C:\ufo\ufo\config\ufo\system.yaml") else (echo [WARN] system.yaml not found)
echo Done!
pause
