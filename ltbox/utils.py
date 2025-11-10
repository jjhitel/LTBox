import os
import platform
import re
import shutil
import subprocess
import sys
import time
import zipfile
from datetime import datetime
from pathlib import Path

from ltbox.constants import *

# --- Process Execution ---
def run_command(command, shell=False, check=True, env=None, capture=False):
    env = env or os.environ.copy()
    env['PATH'] = str(TOOLS_DIR) + os.pathsep + str(DOWNLOAD_DIR) + os.pathsep + env['PATH']

    try:
        process = subprocess.run(
            command, shell=shell, check=check, capture_output=capture,
            text=True, encoding='utf-8', errors='ignore', env=env
        )

        if not capture:
            if process.stdout:
                print(process.stdout.strip())
            if process.stderr:
                print(process.stderr.strip(), file=sys.stderr)
        
        return process
    except FileNotFoundError as e:
        print(f"Error: Command not found - {e.filename}", file=sys.stderr)
        raise
    except subprocess.CalledProcessError as e:
        print(f"Error executing command: {' '.join(map(str, command))}", file=sys.stderr)
        print(f"Return code: {e.returncode}", file=sys.stderr)
        if e.stdout:
            print(f"Stdout:\n{e.stdout.strip()}", file=sys.stderr)
        if e.stderr:
            print(f"Stderr:\n{e.stderr.strip()}", file=sys.stderr)
        raise

# --- Platform & Executable Helpers ---
def get_platform_executable(name):
    system = platform.system()
    executables = {
        "Windows": f"{name}.exe",
        "Linux": f"{name}-linux",
        "Darwin": f"{name}-macos"
    }
    exe_name = executables.get(system)
    if not exe_name:
        raise RuntimeError(f"Unsupported operating system: {system}")
    return DOWNLOAD_DIR / exe_name

# --- File/Directory Waiters ---
def wait_for_files(directory, required_files, prompt_message):
    directory.mkdir(exist_ok=True)
    while True:
        all_found = True
        missing = []
        for file in required_files:
            if not (directory / file).exists():
                all_found = False
                missing.append(file)
        
        if all_found:
            return True
        
        if platform.system() == "Windows":
            os.system('cls')
        else:
            os.system('clear')
            
        print("--- WAITING FOR FILES ---")
        print(prompt_message)
        print(f"\nPlease place the following file(s) in the '{directory.name}' folder:")
        for f in missing:
            print(f" - {f}")
        print("\nPress Enter when ready...")
        try:
            input()
        except EOFError:
            sys.exit(1)

def wait_for_directory(directory, prompt_message):
    directory.mkdir(exist_ok=True)
    while True:
        if directory.is_dir() and any(directory.iterdir()):
             return True
        
        if platform.system() == "Windows":
            os.system('cls')
        else:
            os.system('clear')
            
        print("--- WAITING FOR FOLDER ---")
        print(prompt_message)
        print(f"\nPlease copy the entire folder into this directory:")
        print(f" - {directory.name}{os.sep}")
        print("\nThis is typically located at:")
        print(r"   C:\ProgramData\RSA\Download\RomFiles\[Your_Firmware_Folder]")
        print("\nPress Enter when ready...")
        try:
            input()
        except EOFError:
            sys.exit(1)

# --- Dependency Check ---
def check_dependencies():
    print("--- Checking for required files ---")
    dependencies = {
        "Python Environment": PYTHON_EXE,
        "ADB": ADB_EXE,
        "Fastboot": FASTBOOT_EXE,
        "RSA4096 Key": KEY_MAP["2597c218aae470a130f61162feaae70afd97f011"],
        "RSA2048 Key": KEY_MAP["cdbb77177f731920bbe0a0f94f84d9038ae0617d"],
        "avbtool": AVBTOOL_PY,
        "fetch tool": get_platform_executable("fetch"),
        "edl-ng": EDL_NG_EXE,
        "libusb": LIBUSB_DLL
    }
    missing_deps = [name for name, path in dependencies.items() if not Path(path).exists()]

    if missing_deps:
        for name in missing_deps:
            print(f"[!] Error: Dependency '{name}' is missing.")
        print("Please run one of the main scripts (e.g., root.bat) to install all required files.")
        sys.exit(1)

    print("[+] All dependencies are present.\n")

# --- Info Display ---
def show_image_info(files):
    all_files = []
    for f in files:
        path = Path(f)
        if path.is_dir():
            all_files.extend(path.rglob('*.img'))
        elif path.is_file():
            all_files.append(path)

    if not all_files:
        print("No .img files found in the provided paths.")
        return
        
    output_lines = [
        "\n" + "=" * 42,
        "  Sorted and Processing Images...",
        "=" * 42 + "\n"
    ]
    print("\n".join(output_lines))

    for file_path in sorted(all_files):
        info_header = f"Processing file: {file_path}\n---------------------------------" 
        print(info_header)
        output_lines.append(info_header)

        if not file_path.exists():
            not_found_msg = f"File not found: {file_path}"
            print(not_found_msg)
            output_lines.append(not_found_msg)
            continue

        try:
            process = run_command(
                [str(PYTHON_EXE), str(AVBTOOL_PY), "info_image", "--image", str(file_path)],
                capture=True
            )
            output_text = process.stdout.strip()
            print(output_text)
            output_lines.append(output_text)
        except (subprocess.CalledProcessError) as e:
            error_message = f"Failed to get info from {file_path.name}"
            print(error_message, file=sys.stderr)
            if e.stderr:
                print(e.stderr.strip(), file=sys.stderr)
            output_lines.append(error_message)
        finally:
            output_lines.append("---------------------------------\n")

    try:
        timestamp = datetime.now().strftime("%Y-%m-%d_%H%M%S")
        output_filename = BASE_DIR / f"image_info_{timestamp}.txt"
        with open(output_filename, "w", encoding="utf-8") as f:
            f.write("\n".join(output_lines))
        print(f"[*] Image info saved to: {output_filename}")
    except IOError as e:
        print(f"[!] Error saving info to file: {e}", file=sys.stderr)

def clean_workspace():
    print("--- Starting Cleanup Process ---")
    print("This will remove all input/output folders and temporary files.")
    print("The 'python3', 'backup', and 'tools/dl' folders will NOT be removed.")
    print("-" * 50)

    folders_to_remove = [
        INPUT_CURRENT_DIR, INPUT_NEW_DIR,
        OUTPUT_DIR, OUTPUT_ROOT_DIR, OUTPUT_DP_DIR, OUTPUT_ANTI_ROLLBACK_DIR,
        WORK_DIR,
        IMAGE_DIR,
        WORKING_DIR,
        OUTPUT_XML_DIR,
    ]
    
    print("[*] Removing directories...")
    for folder in folders_to_remove:
        if folder.exists():
            try:
                shutil.rmtree(folder)
                print(f"  > Removed: {folder.name}{os.sep}")
            except OSError as e:
                print(f"[!] Error removing {folder.name}: {e}", file=sys.stderr)
        else:
            print(f"  > Skipping (not found): {folder.name}{os.sep}")

    print("\n[*] Cleaning up temporary files from 'tools/dl' folder...")
    dl_files_to_remove = [
        "*.zip",
        "*.tar.gz",
    ]
    
    cleaned_dl_files = 0
    for pattern in dl_files_to_remove:
        for f in DOWNLOAD_DIR.glob(pattern):
            try:
                f.unlink()
                print(f"  > Removed temp file: {f.name}")
                cleaned_dl_files += 1
            except OSError as e:
                print(f"[!] Error removing {f.name}: {e}", file=sys.stderr)

    if cleaned_dl_files == 0:
        print("  > No temporary archive files found to clean in 'tools/dl'.")


    print("\n[*] Cleaning up temporary files from root directory...")
    file_patterns_to_remove = [
        "*.bak.img",
        "*.root.img",
        "*prc.img",
        "*modified.img",
        "image_info_*.txt",
        "KernelSU*.apk",
        "devinfo.img", 
        "persist.img", 
        "boot.img", 
        "vbmeta.img",
        "platform-tools.zip"
    ]
    
    cleaned_root_files = 0
    for pattern in file_patterns_to_remove:
        for f in BASE_DIR.glob(pattern):
            try:
                f.unlink()
                print(f"  > Removed: {f.name}")
                cleaned_root_files += 1
            except OSError as e:
                print(f"[!] Error removing {f.name}: {e}", file=sys.stderr)
    
    if cleaned_root_files == 0:
        print("  > No temporary files found to clean.")

    print("\n--- Cleanup Finished ---")