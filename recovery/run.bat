@echo off
rem run.bat - start IR Blaster Recovery, building it first if needed (install.bat).
rem
rem   run.bat                 open the window
rem   run.bat --help          command-line options (--list, --info, --recover ...)
setlocal
cd /d "%~dp0"
if not exist dist\irblaster-recovery.exe (
  call install.bat
  if errorlevel 1 exit /b 1
)
rem start /wait keeps the console attached, so command-line output shows before the prompt returns
start "" /wait /b dist\irblaster-recovery.exe %*
endlocal
