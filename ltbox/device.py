import os
import re
import subprocess
import sys
import time
import serial.tools.list_ports
from pathlib import Path
from typing import Optional, List, Dict

from ltbox.constants import *
from ltbox import utils

class DeviceController:
    def __init__(self, skip_adb: bool = False):
        self.skip_adb = skip_adb
        self.edl_port: Optional[str] = None

    def wait_for_adb(self) -> None:
        if self.skip_adb:
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

    def get_device_model(self) -> Optional[str]:
        if self.skip_adb:
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

    def get_active_slot_suffix(self) -> Optional[str]:
        if self.skip_adb:
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

    def get_active_slot_suffix_from_fastboot(self) -> Optional[str]:
        print("[*] Getting active slot suffix via Fastboot...")
        try:
            result = utils.run_command([str(FASTBOOT_EXE), "getvar", "current-slot"], capture=True, check=False)
            output = result.stderr.strip() + "\n" + result.stdout.strip()
            
            match = re.search(r"current-slot:\s*([a-z]+)", output)
            if match:
                slot = match.group(1).strip()
                if slot in ['a', 'b']:
                    suffix = f"_{slot}"
                    print(f"[+] Found active slot suffix (Fastboot): {suffix}")
                    return suffix
            
            print(f"[!] Warning: Could not get valid slot suffix from Fastboot. (Output snippet: {output.splitlines()[0] if output else 'None'})")
            return None
        except Exception as e:
            print(f"[!] Error getting active slot suffix via Fastboot: {e}", file=sys.stderr)
            return None

    def reboot_to_edl(self) -> None:
        if self.skip_adb:
            print("[!] You requested Skip ADB, so please reboot to EDL manually.")
            return
        print("[*] Attempting to reboot device to EDL mode via ADB...")
        try:
            utils.run_command([str(ADB_EXE), "reboot", "edl"])
            print("[+] Reboot command sent. Please wait for the device to enter EDL mode.")
        except Exception as e:
            print(f"[!] Failed to send reboot command: {e}", file=sys.stderr)
            print("[!] Please reboot to EDL manually if it fails.")

    def reboot_to_bootloader(self) -> None:
        if self.skip_adb:
            print("[!] Skipping ADB connection as requested.")
            return
        print("[*] Attempting to reboot device to Fastboot mode via ADB...")
        try:
            utils.run_command([str(ADB_EXE), "reboot", "bootloader"])
            print("[+] Reboot command sent. Please wait for the device to enter Fastboot mode.")
        except Exception as e:
            print(f"[!] Failed to send reboot command: {e}", file=sys.stderr)
            raise

    def check_fastboot_device(self, silent: bool = False) -> bool:
        if not silent:
            print("[*] Checking for fastboot device...")
        try:
            result = utils.run_command([str(FASTBOOT_EXE), "devices"], capture=True, check=False)
            output = result.stdout.strip()
            
            if output:
                if not silent:
                    print(f"[+] Fastboot device found:\n{output}")
                return True
            
            if not silent:
                print("[!] No fastboot device found.")
                print("[!] Please connect your device in fastboot/bootloader mode.")
            return False
        
        except Exception as e:
            if not silent:
                print(f"[!] Error checking for fastboot device: {e}", file=sys.stderr)
            return False

    def wait_for_fastboot(self) -> bool:
        print("\n--- WAITING FOR FASTBOOT DEVICE ---")
        if self.check_fastboot_device(silent=True):
            print("[+] Fastboot device connected.")
            return True
        
        while not self.check_fastboot_device(silent=True):
            print("[*] Waiting for fastboot device... (Press Ctrl+C to cancel)")
            try:
                time.sleep(2)
            except KeyboardInterrupt:
                print("\n[!] Fastboot wait cancelled by user.")
                raise
        print(f"[+] Fastboot device connected.")
        return True

    def fastboot_reboot_system(self) -> None:
        print("[*] Attempting to reboot device to System via Fastboot...")
        try:
            utils.run_command([str(FASTBOOT_EXE), "reboot"])
            print("[+] Reboot command sent.")
        except Exception as e:
            print(f"[!] Failed to send reboot command: {e}", file=sys.stderr)
            
    def get_fastboot_vars(self) -> str:
        print("\n" + "="*61)
        print("  Rollback Check (Fastboot)")
        print("="*61)

        if not self.skip_adb:
            print("  Rebooting to Fastboot mode...")
            self.reboot_to_bootloader()
            print("[*] Waiting for 10 seconds for device to enter Fastboot mode...")
            time.sleep(10)
        else:
            print("[!] Skip ADB is ON.")
            print("[!] Please manually reboot your device to Fastboot mode.")
            print("[!] Press Enter when the device is in Fastboot mode...")
            try:
                input()
            except EOFError:
                pass
        
        self.wait_for_fastboot()
        
        print("[*] Reading rollback indices via fastboot...")
        try:
            result = utils.run_command([str(FASTBOOT_EXE), "getvar", "all"], capture=True, check=False)
            output = result.stdout + "\n" + result.stderr
            
            if not self.skip_adb:
                print("[*] Rebooting back to system...")
                self.fastboot_reboot_system()
            else:
                print("[!] Skip ADB is ON. Leaving device in Fastboot mode.")
                print("[!] (You may need to manually reboot to EDL or System for the next steps)")
            
            return output
        except Exception as e:
            print(f"[!] Failed to get fastboot variables: {e}", file=sys.stderr)
            
            if not self.skip_adb:
                print("[!] Attempting to reboot system anyway...")
                try:
                    self.fastboot_reboot_system()
                except Exception:
                    pass
            raise

    def check_edl_device(self, silent: bool = False) -> Optional[str]:
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

    def wait_for_edl(self) -> str:
        print("\n--- WAITING FOR EDL DEVICE ---")
        port_name = self.check_edl_device()
        if port_name:
            return port_name
        
        while not (port_name := self.check_edl_device(silent=True)):
            print("[*] Waiting for Qualcomm EDL (9008) device... (Press Ctrl+C to cancel)")
            try:
                time.sleep(2)
            except KeyboardInterrupt:
                print("\n[!] EDL wait cancelled by user.")
                raise
        print(f"[+] EDL device connected on {port_name}.")
        return port_name

    def setup_edl_connection(self) -> str:
        if self.check_edl_device(silent=True):
            print("[+] Device is already in EDL mode. Skipping ADB reboot.")
        else:
            if not self.skip_adb:
                self.wait_for_adb()
            
            print("\n--- [EDL Setup] Rebooting to EDL Mode ---")
            self.reboot_to_edl()
            
            if not self.skip_adb:
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

        port = self.wait_for_edl()
        self.edl_port = port
        print("--- [EDL Setup] Device Connected ---")
        return port

    def load_firehose_programmer(self, loader_path: Path, port: str) -> None:
        if not QSAHARASERVER_EXE.exists():
            raise FileNotFoundError(f"QSaharaServer.exe not found at {QSAHARASERVER_EXE}")
            
        port_str = f"\\\\.\\{port}"
        print(f"[*] Uploading programmer via QSaharaServer to {port}...")
        
        cmd_sahara = [
            str(QSAHARASERVER_EXE),
            "-p", port_str,
            "-s", f"13:{loader_path}"
        ]
        
        try:
            utils.run_command(cmd_sahara, check=True)
        except subprocess.CalledProcessError as e:
            print(f"\n[!] FATAL ERROR: Failed to load Firehose programmer.", file=sys.stderr)
            print(f"[!] Possible causes:", file=sys.stderr)
            print(f"    1. Connection instability (Try a different USB cable/port).", file=sys.stderr)
            print(f"    2. Driver issue (Check Qualcomm HS-USB QDLoader 9008 in Device Manager).", file=sys.stderr)
            print(f"    3. Device is hung (Hold Power+Vol- for 10s to force reboot, then try again).", file=sys.stderr)
            raise e

    def fh_loader_read_part(
        self,
        port: str, 
        output_filename: str, 
        lun: str, 
        start_sector: str, 
        num_sectors: str, 
        memory_name: str = "UFS"
    ) -> None:
        if not FH_LOADER_EXE.exists():
            raise FileNotFoundError(f"fh_loader.exe not found at {FH_LOADER_EXE}")

        dest_file = Path(output_filename).resolve()
        dest_dir = dest_file.parent
        dest_filename = dest_file.name
        
        dest_dir.mkdir(parents=True, exist_ok=True)

        env = os.environ.copy()
        env['PATH'] = str(TOOLS_DIR) + os.pathsep + str(DOWNLOAD_DIR) + os.pathsep + env['PATH']

        port_str = f"\\\\.\\{port}"
        cmd_fh = [
            str(FH_LOADER_EXE),
            f"--port={port_str}",
            "--convertprogram2read",
            f"--sendimage={dest_filename}",
            f"--lun={lun}",
            f"--start_sector={start_sector}",
            f"--num_sectors={num_sectors}",
            f"--memoryname={memory_name}",
            "--noprompt",
            "--zlpawarehost=1"
        ]
        
        print(f"[*] Dumping -> LUN:{lun}, Start:{start_sector}, Num:{num_sectors}...")
        
        try:
            subprocess.run(cmd_fh, cwd=dest_dir, env=env, check=True)
        except subprocess.CalledProcessError as e:
            print(f"[!] Error executing fh_loader: {e}", file=sys.stderr)
            raise

    def fh_loader_write_part(
        self,
        port: str, 
        image_path: Path, 
        lun: str, 
        start_sector: str, 
        memory_name: str = "UFS"
    ) -> None:
        if not FH_LOADER_EXE.exists():
            raise FileNotFoundError(f"fh_loader.exe not found at {FH_LOADER_EXE}")

        image_file = Path(image_path).resolve()
        work_dir = image_file.parent
        filename = image_file.name
        
        port_str = f"\\\\.\\{port}"
        
        cmd_fh = [
            str(FH_LOADER_EXE),
            f"--port={port_str}",
            f"--sendimage={filename}",
            f"--lun={lun}",
            f"--start_sector={start_sector}",
            f"--memoryname={memory_name}",
            "--noprompt",
            "--zlpawarehost=1"
        ]
        
        print(f"[*] Flashing -> {filename} to LUN:{lun}, Start:{start_sector}...")
        
        env = os.environ.copy()
        env['PATH'] = str(TOOLS_DIR) + os.pathsep + str(DOWNLOAD_DIR) + os.pathsep + env['PATH']

        try:
            subprocess.run(cmd_fh, cwd=work_dir, env=env, check=True)
            print(f"[+] Successfully flashed '{filename}'.")
        except subprocess.CalledProcessError as e:
            print(f"[!] Error executing fh_loader write: {e}", file=sys.stderr)
            raise

    def fh_loader_reset(self, port: str) -> None:
        if not FH_LOADER_EXE.exists():
            raise FileNotFoundError(f"fh_loader.exe not found at {FH_LOADER_EXE}")
            
        port_str = f"\\\\.\\{port}"
        print(f"[*] Resetting device via fh_loader on {port}...")
        
        cmd_fh = [
            str(FH_LOADER_EXE),
            f"--port={port_str}",
            "--reset",
            "--noprompt"
        ]
        utils.run_command(cmd_fh)

    def edl_rawprogram(
        self,
        loader_path: Path, 
        memory_type: str, 
        raw_xmls: List[Path], 
        patch_xmls: List[Path], 
        port: str
    ) -> None:
        if not QSAHARASERVER_EXE.exists() or not FH_LOADER_EXE.exists():
            print(f"[!] Error: Qsaharaserver.exe or fh_loader.exe not found in {TOOLS_DIR.name} folder.")
            raise FileNotFoundError("Missing fh_loader/Qsaharaserver executables")
        
        port_str = f"\\\\.\\{port}"
        search_path = str(loader_path.parent)

        print("[*] STEP 1/2: Loading programmer with Qsaharaserver...")
        self.load_firehose_programmer(loader_path, port)

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