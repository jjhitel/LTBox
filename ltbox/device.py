import platform
import subprocess
import sys
import time
import zipfile
import shutil
import serial.tools.list_ports

from ltbox.constants import *
from ltbox import utils

# --- ADB Device Handling ---
def wait_for_adb(skip_adb=False):
    if skip_adb:
        print("[!] Skipping ADB connection as requested.")
        return
    print("\n--- WAITING FOR ADB DEVICE ---")
    print("[!] Please enable USB Debugging on your device, connect it via USB.")
    print("[!] A 'Allow USB debugging?' prompt will appear on your device.")
    print("[!] Please check 'Always allow from this computer' and tap 'OK'.")
    try:
        utils.run_command([str(ADB_EXE), "wait-for-device"])
        print("[+] ADB device connected.")
    except Exception as e:
        print(f"[!] Error waiting for ADB device: {e}", file=sys.stderr)
        raise

def get_device_model(skip_adb=False):
    if skip_adb:
        print("[!] Skipping device model check as requested.")
        return None
    print("[*] Getting device model via ADB...")
    try:
        result = utils.run_command([str(ADB_EXE), "shell", "getprop", "ro.product.model"], capture=True)
        model = result.stdout.strip()
        if not model:
            print("[!] Could not get device model. Is the device authorized?")
            return None
        print(f"[+] Found device model: {model}")
        return model
    except Exception as e:
        print(f"[!] Error getting device model: {e}", file=sys.stderr)
        print("[!] Please ensure the device is connected and authorized.")
        return None

def get_active_slot_suffix(skip_adb=False):
    if skip_adb:
        print("[!] Skipping active slot check as requested.")
        return None
    print("[*] Getting active slot suffix via ADB...")
    try:
        result = utils.run_command([str(ADB_EXE), "shell", "getprop", "ro.boot.slot_suffix"], capture=True)
        suffix = result.stdout.strip()
        if suffix not in ["_a", "_b"]:
            print(f"[!] Warning: Could not get valid slot suffix (got '{suffix}'). Assuming non-A/B device.")
            return None
        print(f"[+] Found active slot suffix: {suffix}")
        return suffix
    except Exception as e:
        print(f"[!] Error getting active slot suffix: {e}", file=sys.stderr)
        print("[!] Please ensure the device is connected and authorized.")
        return None

def reboot_to_edl(skip_adb=False):
    if skip_adb:
        print("[!] You requested Skip ADB, so please reboot to EDL manually.")
        return
    print("[*] Attempting to reboot device to EDL mode via ADB...")
    try:
        utils.run_command([str(ADB_EXE), "reboot", "edl"])
        print("[+] Reboot command sent. Please wait for the device to enter EDL mode.")
    except Exception as e:
        print(f"[!] Failed to send reboot command: {e}", file=sys.stderr)
        print("[!] Please reboot to EDL manually if it fails.")

def reboot_to_bootloader(skip_adb=False):
    if skip_adb:
        print("[!] Skipping ADB connection as requested.")
        return
    print("[*] Attempting to reboot device to Bootloader mode via ADB...")
    try:
        utils.run_command([str(ADB_EXE), "reboot", "bootloader"])
        print("[+] Reboot command sent. Please wait for the device to enter bootloader mode.")
    except Exception as e:
        print(f"[!] Failed to send reboot command: {e}", file=sys.stderr)
        raise

# --- Fastboot Device Handling ---
def wait_for_fastboot():
    print("\n--- WAITING FOR FASTBOOT DEVICE ---")
    try:
        utils.run_command([str(FASTBOOT_EXE), "wait-for-device"])
        print("[+] Fastboot device connected.")
    except Exception as e:
        print(f"[!] Error waiting for Fastboot device: {e}", file=sys.stderr)
        raise

def fastboot_reboot_system():
    print("[*] Attempting to reboot device to System via Fastboot...")
    try:
        utils.run_command([str(FASTBOOT_EXE), "reboot"])
        print("[+] Reboot command sent.")
    except Exception as e:
        print(f"[!] Failed to send reboot command: {e}", file=sys.stderr)
        
def get_fastboot_vars(skip_adb=False):
    if skip_adb:
        print("[!] Skipping fastboot operations as requested by Skip ADB setting.")
        return None
    
    print("\n" + "="*61)
    print("  Rebooting to Bootloader for Rollback Check")
    print("="*61)
    reboot_to_bootloader(skip_adb=skip_adb)
    print("[*] Waiting for 10 seconds for device to enter bootloader mode...")
    time.sleep(10)
    
    wait_for_fastboot()
    
    print("[*] Reading rollback indices via fastboot...")
    try:
        result = utils.run_command([str(FASTBOOT_EXE), "getvar", "all"], capture=True, check=False)
        output = result.stdout + "\n" + result.stderr
        
        print("[*] Rebooting back to system...")
        fastboot_reboot_system()
        
        return output
    except Exception as e:
        print(f"[!] Failed to get fastboot variables: {e}", file=sys.stderr)
        print("[!] Attempting to reboot system anyway...")
        try:
            fastboot_reboot_system()
        except Exception:
            pass
        raise
        
# --- EDL Device Handling ---
def _ensure_edl_ng():
    if platform.system() != "Windows":
        print("[!] EDL functions are only supported on Windows.", file=sys.stderr)
        sys.exit(1)
        
    edl_ng_exe = TOOLS_DIR / "edl-ng.exe"
    if edl_ng_exe.exists():
        return edl_ng_exe

    fetch_exe = utils.get_platform_executable("fetch")
    if not fetch_exe.exists():
         print(f"[!] '{fetch_exe.name}' not found. Please run install.bat")
         sys.exit(1)

    print(f"[!] '{edl_ng_exe.name}' not found. Attempting to download...")
    arch = platform.machine()
    if arch == 'AMD64':
        asset_pattern = "edl-ng-windows-x64.zip"
    elif arch == 'ARM64':
        asset_pattern = "edl-ng-windows-arm64.zip"
    else:
        print(f"[!] Unsupported Windows architecture: {arch}. Cannot download edl-ng.", file=sys.stderr)
        sys.exit(1)
        
    print(f"[*] Detected {arch} architecture. Downloading '{asset_pattern}'...")

    try:
        fetch_command = [
            str(fetch_exe),
            "--repo", EDL_NG_REPO_URL,
            "--tag", EDL_NG_TAG,
            "--release-asset", asset_pattern,
            str(TOOLS_DIR)
        ]
        utils.run_command(fetch_command, capture=True)

        downloaded_zip_path = TOOLS_DIR / asset_pattern
        
        if not downloaded_zip_path.exists():
            raise FileNotFoundError(f"Failed to find the downloaded edl-ng zip archive: {asset_pattern}")
        
        with zipfile.ZipFile(downloaded_zip_path, 'r') as zip_ref:
            edl_info = None
            libusb_info = None
            
            for member in zip_ref.infolist():
                if member.filename.endswith('edl-ng.exe'):
                    edl_info = member
                elif member.filename.endswith('libusb-1.0.dll'):
                    libusb_info = member
            
            if not edl_info:
                raise FileNotFoundError("edl-ng.exe not found inside the downloaded zip archive.")

            zip_ref.extract(edl_info, path=TOOLS_DIR)
            
            extracted_path = TOOLS_DIR / edl_info.filename
            if extracted_path != edl_ng_exe:
                shutil.move(extracted_path, edl_ng_exe)
            
            extracted_libusb_path = None
            if libusb_info:
                libusb_dll_path = TOOLS_DIR / "libusb-1.0.dll"
                zip_ref.extract(libusb_info, path=TOOLS_DIR)
                extracted_libusb_path = TOOLS_DIR / libusb_info.filename
                if extracted_libusb_path != libusb_dll_path:
                    shutil.move(extracted_libusb_path, libusb_dll_path)

            parent_dir = extracted_path.parent
            if parent_dir.is_dir() and parent_dir != TOOLS_DIR:
                try:
                    parent_dir.rmdir()
                except OSError:
                    shutil.rmtree(parent_dir, ignore_errors=True)

            if extracted_libusb_path:
                libusb_parent_dir = extracted_libusb_path.parent
                if libusb_parent_dir.is_dir() and libusb_parent_dir != TOOLS_DIR and libusb_parent_dir != parent_dir:
                    try:
                        libusb_parent_dir.rmdir()
                    except OSError:
                        shutil.rmtree(libusb_parent_dir, ignore_errors=True)


        downloaded_zip_path.unlink()
        print("[+] edl-ng download and extraction successful.")
        return edl_ng_exe

    except (subprocess.CalledProcessError, FileNotFoundError, KeyError, IndexError) as e:
        print(f"[!] Error downloading or extracting edl-ng: {e}", file=sys.stderr)
        sys.exit(1)

def check_edl_device(silent=False):
    if not silent:
        print("[*] Checking for Qualcomm EDL (9008) device...")
    
    try:
        ports = serial.tools.list_ports.comports()
        for port in ports:
            is_qualcomm_port = (port.description and "Qualcomm" in port.description and "9008" in port.description) or \
                               (port.hwid and "VID:PID=05C6:9008" in port.hwid.upper())
            
            if is_qualcomm_port:
                if not silent:
                    print(f"[+] Qualcomm EDL device found: {port.device}")
                return port.device
        
        if not silent:
            print("[!] No Qualcomm EDL (9008) device found.")
            print("[!] Please connect your device in EDL mode.")
        return None
    
    except Exception as e:
        if not silent:
            print(f"[!] Error checking for EDL device: {e}", file=sys.stderr)
        return None

def wait_for_edl():
    print("\n--- WAITING FOR EDL DEVICE ---")
    port_name = check_edl_device()
    if port_name:
        return port_name
    
    while not (port_name := check_edl_device(silent=True)):
        print("[*] Waiting for Qualcomm EDL (9008) device... (Press Ctrl+C to cancel)")
        try:
            time.sleep(2)
        except KeyboardInterrupt:
            print("\n[!] EDL wait cancelled by user.")
            raise
    print(f"[+] EDL device connected on {port_name}.")
    return port_name

def setup_edl_connection(skip_adb=False):
    print("\n--- [EDL Setup] Rebooting to EDL Mode ---")
    reboot_to_edl(skip_adb=skip_adb)
    if not skip_adb:
        print("[*] Waiting for 10 seconds for device to enter EDL mode...")
        time.sleep(10)

    print(f"--- [EDL Setup] Waiting for EDL Loader File ---")
    required_files = [EDL_LOADER_FILENAME]
    prompt = (
        f"[STEP 1] Place the EDL loader file ('{EDL_LOADER_FILENAME}')\n"
        f"         into the '{IMAGE_DIR.name}' folder to proceed."
    )
    IMAGE_DIR.mkdir(exist_ok=True)
    utils.wait_for_files(IMAGE_DIR, required_files, prompt)
    print(f"[+] Loader file '{EDL_LOADER_FILE.name}' found in '{IMAGE_DIR.name}'.")

    port = wait_for_edl()
    print("--- [EDL Setup] Device Connected ---")
    return port

def _run_edl_command(loader_path, args_list):
    edl_ng_exe = _ensure_edl_ng()
    base_cmd = [str(edl_ng_exe), "--loader", str(loader_path)]
    base_cmd.extend(args_list)
    return utils.run_command(base_cmd)

def edl_read_part(loader_path, partition, output_file):
    return _run_edl_command(loader_path, ["read-part", partition, str(output_file)])

def edl_write_part(loader_path, partition, input_file):
    return _run_edl_command(loader_path, ["write-part", partition, str(input_file)])

def edl_reset(loader_path, mode=None):
    cmd = ["reset"]
    if mode == "edl":
        cmd.extend(["--mode", "edl"])
    return _run_edl_command(loader_path, cmd)

def edl_rawprogram(loader_path, memory_type, raw_xmls, patch_xmls, port):
    if not QSAHARASERVER_EXE.exists() or not FH_LOADER_EXE.exists():
        print(f"[!] Error: Qsaharaserver.exe or fh_loader.exe not found in {TOOLS_DIR.name} folder.")
        raise FileNotFoundError("Missing fh_loader/Qsaharaserver executables")
    
    port_str = f"\\\\.\\{port}"
    search_path = str(loader_path.parent)

    print("[*] STEP 1/2: Loading programmer with Qsaharaserver...")
    cmd_sahara = [
        str(QSAHARASERVER_EXE), 
        "-p", port_str, 
        "-s", f"13:{loader_path}"
    ]
    utils.run_command(cmd_sahara)

    print("\n[*] STEP 2/2: Flashing firmware with fh_loader...")
    raw_xml_str = ",".join([p.name for p in raw_xmls])
    patch_xml_str = ",".join([p.name for p in patch_xmls])

    cmd_fh = [
        str(FH_LOADER_EXE),
        f"--port={port_str}",
        f"--search_path={search_path}",
        f"--sendxml={raw_xml_str}",
        f"--sendxml={patch_xml_str}",
        "--setactivepartition=1",
        f"--memoryname={memory_type}",
        "--showpercentagecomplete",
        "--zlpawarehost=1",
        "--noprompt"
    ]
    utils.run_command(cmd_fh)