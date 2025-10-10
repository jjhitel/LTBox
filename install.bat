@echo off
setlocal

echo --- Required Files Installer ---
echo.

:: ======================================================
:: Variable Definitions
:: ======================================================
set "TOOLS_DIR=%~dp0tools"
set "KEY_DIR=%~dp0key"
set "PYTHON_DIR=%~dp0python3"
set "PYTHON_VERSION=3.14.0"
set "PYTHON_ZIP_URL=https://www.python.org/ftp/python/%PYTHON_VERSION%/python-%PYTHON_VERSION%-embed-amd64.zip"
set "PYTHON_ZIP_PATH=%~dp0python_embed.zip"
set "PYTHON_PTH_FILE_SRC=%TOOLS_DIR%\\python314._pth"
set "PYTHON_PTH_FILE_DST=%PYTHON_DIR%\\python314._pth"
set "GETPIP_URL=https://bootstrap.pypa.io/get-pip.py"
set "GETPIP_PATH=%PYTHON_DIR%\\get-pip.py"

:: ======================================================
:: Create Directories
:: ======================================================
if not exist "%TOOLS_DIR%" mkdir "%TOOLS_DIR%"
if not exist "%KEY_DIR%" mkdir "%KEY_DIR%"

:: ======================================================
:: 1. Setup Standalone Python Environment
:: ======================================================
echo [*] Checking for Python environment...
if exist "%PYTHON_DIR%\\python.exe" goto PythonExists

echo [!] Python environment not found. Starting setup...

echo [*] Downloading Python embeddable package (%PYTHON_VERSION%)...
curl -L "%PYTHON_ZIP_URL%" -o "%PYTHON_ZIP_PATH%"
if errorlevel 1 (
    echo [!] Failed to download Python. Please check your internet connection.
    pause
    exit /b
)

echo [*] Extracting Python...
powershell -Command "Expand-Archive -Path '%PYTHON_ZIP_PATH%' -DestinationPath '%PYTHON_DIR%' -Force"
if errorlevel 1 (
    echo [!] Failed to extract Python. Make sure you have PowerShell.
    del "%PYTHON_ZIP_PATH%"
    pause
    exit /b
)

del "%PYTHON_ZIP_PATH%"
echo [+] Python environment created successfully.

echo [*] Configuring Python environment...
if exist "%PYTHON_PTH_FILE_SRC%" (
    copy "%PYTHON_PTH_FILE_SRC%" "%PYTHON_PTH_FILE_DST%" >nul
) else (
    echo import site > "%PYTHON_PTH_FILE_DST%"
)

echo [*] Downloading get-pip.py...
curl -L "%GETPIP_URL%" -o "%GETPIP_PATH%"
if errorlevel 1 (
    echo [!] Failed to download get-pip.py.
    pause
    exit /b
)

echo [*] Installing pip...
"%PYTHON_DIR%\\python.exe" "%GETPIP_PATH%"
if errorlevel 1 (
    echo [!] Failed to install pip.
    del "%GETPIP_PATH%"
    pause
    exit /b
)

del "%GETPIP_PATH%"
echo [+] pip installed successfully.

echo [*] Installing cryptography package...
"%PYTHON_DIR%\\python.exe" -m pip install cryptography
if errorlevel 1 (
    echo [!] Failed to install cryptography.
    pause
    exit /b
)
echo [+] cryptography installed successfully.


echo [*] Installing requests package...
"%PYTHON_DIR%\\python.exe" -m pip install requests
if errorlevel 1 (
    echo [!] Failed to install requests.
    pause
    exit /b
)
echo [+] requests installed successfully.

:PythonExists
echo [+] Python environment is ready.
echo.

:: ======================================================
:: 2. Download Other Required Files
:: ======================================================

echo --- Downloading Tools ---
echo.

echo [*] Checking for avbtool.py...
if exist "%TOOLS_DIR%\\avbtool.py" goto avbtool_exists
echo [!] 'avbtool.py' not found. Attempting to download...
curl -L "https://github.com/LineageOS/android_external_avb/raw/refs/heads/lineage-22.2/avbtool.py" -o "%TOOLS_DIR%\\avbtool.py"
if exist "%TOOLS_DIR%\\avbtool.py" (echo [+] Download successful.) else (echo [!] Download failed.)
:avbtool_exists
echo.

echo [*] Checking for testkey_rsa4096.pem...
if exist "%KEY_DIR%\\testkey_rsa4096.pem" goto key4096_exists
echo [!] 'testkey_rsa4096.pem' not found. Attempting to download...
curl -L "https://github.com/LineageOS/android_external_avb/raw/refs/heads/lineage-22.2/test/data/testkey_rsa4096.pem" -o "%KEY_DIR%\\testkey_rsa4096.pem"
if exist "%KEY_DIR%\\testkey_rsa4096.pem" (echo [+] Download successful.) else (echo [!] Download failed.)
:key4096_exists
echo.

echo [*] Checking for testkey_rsa2048.pem...
if exist "%KEY_DIR%\\testkey_rsa2048.pem" goto key2048_exists
echo [!] 'testkey_rsa2048.pem' not found. Attempting to download...
curl -L "https://github.com/LineageOS/android_external_avb/raw/refs/heads/lineage-22.2/test/data/testkey_rsa2048.pem" -o "%KEY_DIR%\\testkey_rsa2048.pem"
if exist "%KEY_DIR%\\testkey_rsa2048.pem" (echo [+] Download successful.) else (echo [!] Download failed.)
:key2048_exists
echo.


:: ======================================================
:: Finalization
:: ======================================================
echo --- All required files are present ---
echo.
pause