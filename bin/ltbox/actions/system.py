import subprocess
from typing import Optional, Dict

from .. import constants as const
from .. import utils, device
from ..i18n import get_string
from ..errors import ToolError

def detect_active_slot_robust(dev: device.DeviceController) -> Optional[str]:
    active_slot = None

    if not dev.skip_adb:
        try:
            active_slot = dev.get_active_slot_suffix()
        except Exception:
            pass

    if not active_slot:
        if not dev.skip_adb:
            try:
                dev.reboot_to_bootloader()
            except Exception as e:
                raise ToolError(get_string("act_err_reboot_bl").format(e=e))
        else:
            print("\n" + "="*60)
            print(get_string("act_manual_fastboot"))
            print("="*60 + "\n")


        dev.wait_for_fastboot()
        active_slot = dev.get_active_slot_suffix_from_fastboot()

        if not dev.skip_adb:
            dev.fastboot_reboot_system()
            dev.wait_for_adb()
        else:
            print("\n" + "="*60)
            print(get_string("act_detect_complete"))
            print(get_string("act_manual_edl"))
            print("="*60 + "\n")

    return active_slot

def disable_ota(dev: device.DeviceController) -> str:
    if dev.skip_adb:
        raise ToolError(get_string("act_ota_skip_adb"))
    
    dev.wait_for_adb()
    
    command = [
        str(const.ADB_EXE), 
        "shell", "pm", "disable-user", "--user", "0", "com.lenovo.ota"
    ]
    
    try:
        result = utils.run_command(command, capture=True)
        if "disabled" in result.stdout.lower() or "already disabled" in result.stdout.lower():
            success_msg = get_string("act_ota_disabled")
            success_msg += f"\n{result.stdout.strip()}"
            return success_msg
        else:
            err_msg = get_string("act_ota_unexpected")
            err_msg += f"\n{get_string('act_ota_stdout').format(output=result.stdout.strip())}"
            if result.stderr:
                err_msg += f"\n{get_string('act_ota_stderr').format(output=result.stderr.strip())}"
            raise ToolError(err_msg)
    except Exception as e:
        raise ToolError(get_string("act_err_ota_cmd").format(e=e))