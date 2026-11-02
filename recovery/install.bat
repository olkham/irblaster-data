@echo off
rem install.bat - build IR Blaster Recovery from source into recovery\dist\ (Windows).
rem
rem   install.bat           build or rebuild the app; safe to run again
rem   install.bat --clean   delete earlier builds (target\ and dist\) and build from scratch
rem   install.bat --help
rem
rem Then start it with run.bat. Needs Rust and the Visual Studio C++ build tools; this script offers
rem to install both with winget.
setlocal EnableExtensions
cd /d "%~dp0"

set "CLEAN=0"
:args
if "%~1"=="" goto args_done
if /i "%~1"=="--clean" (set "CLEAN=1" & shift & goto args)
if /i "%~1"=="--help" goto usage
if /i "%~1"=="-h" goto usage
echo error: unknown option "%~1" (see --help) 1>&2
exit /b 2
:usage
echo Usage: install.bat [--clean]
echo   (no option)  build or rebuild the app into dist\
echo   --clean      delete earlier builds and build from scratch
exit /b 0
:args_done

rem --- Rust -------------------------------------------------------------------------------
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>&1
if not errorlevel 1 goto have_rust
echo Rust is not installed. It is needed to build the app.
where winget >nul 2>&1
if errorlevel 1 (
  echo Install it from https://rustup.rs, then run install.bat again.
  exit /b 1
)
set "ANSWER="
set /p "ANSWER=Install it now with winget (Rustlang.Rustup)? [y/N] "
if /i not "%ANSWER%"=="y" if /i not "%ANSWER%"=="yes" (
  echo Install it from https://rustup.rs, then run install.bat again.
  exit /b 1
)
winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
if errorlevel 1 exit /b 1
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
rustup default stable
:have_rust
rustup update stable >nul 2>&1

rem --- MSVC linker --------------------------------------------------------------------------
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
set "HAVE_VC="
if exist "%VSWHERE%" (
  for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "HAVE_VC=%%i"
)
if defined HAVE_VC goto have_vc
echo The Visual Studio C++ build tools are needed to link the app (about 2 GB).
where winget >nul 2>&1
if errorlevel 1 (
  echo Install "Build Tools for Visual Studio" with the "Desktop development with C++" workload, then run install.bat again.
  exit /b 1
)
set "ANSWER="
set /p "ANSWER=Install them now with winget? [y/N] "
if /i not "%ANSWER%"=="y" if /i not "%ANSWER%"=="yes" (
  echo Install "Build Tools for Visual Studio" with the "Desktop development with C++" workload, then run install.bat again.
  exit /b 1
)
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-package-agreements --accept-source-agreements --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
if errorlevel 1 exit /b 1
:have_vc

rem --- build ------------------------------------------------------------------------------
if "%CLEAN%"=="1" (
  echo ==^> Removing earlier builds
  cargo clean
  if exist dist rmdir /s /q dist
)
echo ==^> Building (the first build downloads and compiles dependencies, a few minutes)
cargo build --release --locked
if errorlevel 1 exit /b 1
if not exist dist mkdir dist
copy /y target\release\irblaster-recovery.exe dist\irblaster-recovery.exe >nul
echo ==^> Built dist\irblaster-recovery.exe
echo.
echo Start it with run.bat (or run.bat --help for the command line).
endlocal
