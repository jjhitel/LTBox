@echo off
chcp 65001 > nul
setlocal

set "SKIP_ADB=0"
set "SKIP_ADB_STATE=OFF"

:: --- 1. Initialization and Dependency Check ---
echo --- Initializing LTBox... ---
call "%~dp0ltbox\install.bat"
if errorlevel 1 (
    echo [!] Dependency installation failed. Please check ltbox\install.bat.
    pause
    goto :eof
)

:: --- 2. Set Python and Main Script Paths ---
set "PYTHON_EXE=%~dp0python3\python.exe"
set "MAIN_PY=%~dp0ltbox\main.py"

if not exist "%PYTHON_EXE%" (
    echo [!] Python not found at: %PYTHON_EXE%
    echo [!] Please run ltbox\install.bat first.
    pause
    goto :eof
)
if not exist "%MAIN_PY%" (
    echo [!] Main script not found at: %MAIN_PY%
    pause
    goto :eof
)

:: --- 3. Run Main Python Script ---
"%PYTHON_EXE%" "%MAIN_PY%"
goto :eof

:: --- 6. Exit ---
:cleanup
endlocal
echo.
echo Exiting LTBox.
goto :eof