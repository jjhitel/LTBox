import re
import shutil
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Optional, List, Dict, Any

from ..constants import *
from .. import utils
from ..crypto import decrypt_file
from ..i18n import get_string

def _modify_xml_algo(wipe: int = 0) -> None:
    def is_garbage_file(path: Path) -> bool:
        name = path.name.lower()
        stem = path.stem.lower()
        if stem == "rawprogram_unsparse0": return True
        if "wipe_partitions" in name or "blank_gpt" in name: return True
        return False

    if OUTPUT_XML_DIR.exists():
        shutil.rmtree(OUTPUT_XML_DIR)
    OUTPUT_XML_DIR.mkdir(parents=True, exist_ok=True)

    print(get_string("img_xml_scan"))
    
    x_files = list(IMAGE_DIR.glob("*.x"))
    xml_files = list(IMAGE_DIR.glob("*.xml"))
    
    processed_files = False

    if x_files:
        print(get_string("img_xml_found_x").format(count=len(x_files), dir=OUTPUT_XML_DIR.name))
        for file in x_files:
            out_file = OUTPUT_XML_DIR / file.with_suffix('.xml').name
            try:
                if decrypt_file(str(file), str(out_file)):
                    print(get_string("img_xml_decrypt_ok").format(src=file.name, dst=out_file.name))
                    processed_files = True
                else:
                    print(get_string("img_xml_decrypt_fail").format(name=file.name))
            except Exception as e:
                print(get_string("img_xml_decrypt_err").format(name=file.name, e=e), file=sys.stderr)

    if xml_files:
        print(get_string("img_xml_found_xml").format(count=len(xml_files), dir=OUTPUT_XML_DIR.name))
        for file in xml_files:
            out_file = OUTPUT_XML_DIR / file.name
            try:
                if out_file.exists():
                    out_file.unlink()
                shutil.move(str(file), str(out_file))
                print(get_string("img_xml_moved").format(name=file.name))
                processed_files = True
            except Exception as e:
                print(get_string("img_xml_move_err").format(name=file.name, e=e), file=sys.stderr)

    if not processed_files:
        print(get_string("img_xml_no_files").format(dir=IMAGE_DIR.name))
        shutil.rmtree(OUTPUT_XML_DIR)
        raise FileNotFoundError(f"No .x or .xml files in {IMAGE_DIR.name}")

    rawprogram4 = OUTPUT_XML_DIR / "rawprogram4.xml"
    rawprogram_unsparse4 = OUTPUT_XML_DIR / "rawprogram_unsparse4.xml"
    
    if not rawprogram4.exists() and rawprogram_unsparse4.exists():
        print(get_string("img_xml_copy_raw4"))
        shutil.copy(rawprogram_unsparse4, rawprogram4)

    print(get_string("img_xml_mod_raw"))
    
    rawprogram_save = OUTPUT_XML_DIR / "rawprogram_save_persist_unsparse0.xml"

    if not rawprogram_save.exists():
        rawprogram_fallback = OUTPUT_XML_DIR / "rawprogram_unsparse0-half.xml"
        
        if rawprogram_fallback.exists():
            print(get_string("img_xml_rename_fallback").format(target=rawprogram_save.name, src=rawprogram_fallback.name))
            try:
                rawprogram_fallback.rename(rawprogram_save)
            except OSError as e:
                print(get_string("img_xml_rename_err").format(e=e), file=sys.stderr)
                raise
        else:
            print(get_string("img_xml_critical_missing").format(f1=rawprogram_save.name, f2=rawprogram_fallback.name))
            print(get_string("img_xml_abort_mod"))
            raise FileNotFoundError(f"Critical XML file missing: {rawprogram_save.name} or {rawprogram_fallback.name}")

    try:
        with open(rawprogram_save, 'r', encoding='utf-8') as f:
            content = f.read()
        
        if wipe == 0:
            print(get_string("img_xml_nowipe"))
            for i in range(1, 11):
                content = content.replace(f'filename="metadata_{i}.img"', '')
            for i in range(1, 21):
                content = content.replace(f'filename="userdata_{i}.img"', '')
        else:
            print(get_string("img_xml_wipe"))
            
        with open(rawprogram_save, 'w', encoding='utf-8') as f:
            f.write(content)
        print(get_string("img_xml_patch_ok"))
    except Exception as e:
        print(get_string("img_xml_patch_err").format(e=e), file=sys.stderr)
        raise

    print(get_string("img_xml_cleanup"))
    
    files_to_delete = []
    for f in OUTPUT_XML_DIR.glob("*.xml"):
        if is_garbage_file(f):
            files_to_delete.append(f)

    if files_to_delete:
        for f in files_to_delete:
            try:
                f.unlink()
                print(get_string("img_xml_deleted").format(name=f.name))
            except OSError as e:
                print(get_string("img_xml_del_err").format(name=f.name, e=e))
    else:
        print(get_string("img_xml_no_del"))

    print(get_string("img_xml_complete").format(dir=OUTPUT_XML_DIR.name))


def modify_xml(wipe: int = 0, skip_dp: bool = False) -> None:
    print(get_string("act_start_xml_mod"))
    
    print(get_string("act_wait_image"))
    prompt = get_string("act_prompt_image")
    utils.wait_for_directory(IMAGE_DIR, prompt)

    if OUTPUT_XML_DIR.exists():
        shutil.rmtree(OUTPUT_XML_DIR)
    OUTPUT_XML_DIR.mkdir(exist_ok=True)

    with utils.temporary_workspace(WORKING_DIR):
        print(get_string("act_create_temp").format(dir=WORKING_DIR.name))
        try:
            _modify_xml_algo(wipe=wipe)

            if not skip_dp:
                print(get_string("act_create_write_xml"))

                src_persist_xml = OUTPUT_XML_DIR / "rawprogram_save_persist_unsparse0.xml"
                dest_persist_xml = OUTPUT_XML_DIR / "rawprogram_write_persist_unsparse0.xml"
                
                if src_persist_xml.exists():
                    try:
                        content = src_persist_xml.read_text(encoding='utf-8')
                        
                        content = re.sub(
                            r'(<program[^>]*\blabel="persist"[^>]*filename=")[^"]*(".*/>)',
                            r'\1persist.img\2',
                            content,
                            flags=re.IGNORECASE
                        )
                        content = re.sub(
                            r'(<program[^>]*filename=")[^"]*("[^>]*\blabel="persist"[^>]*/>)',
                            r'\1persist.img\2',
                            content,
                            flags=re.IGNORECASE
                        )
                        
                        dest_persist_xml.write_text(content, encoding='utf-8')
                        print(get_string("act_created_persist_xml").format(name=dest_persist_xml.name, parent=dest_persist_xml.parent.name))
                    except Exception as e:
                        print(get_string("act_err_create_persist_xml").format(name=dest_persist_xml.name, e=e), file=sys.stderr)
                else:
                    print(get_string("act_warn_persist_xml_missing").format(name=src_persist_xml.name))

                src_devinfo_xml = OUTPUT_XML_DIR / "rawprogram4.xml"
                dest_devinfo_xml = OUTPUT_XML_DIR / "rawprogram4_write_devinfo.xml"
                
                if src_devinfo_xml.exists():
                    try:
                        content = src_devinfo_xml.read_text(encoding='utf-8')

                        content = re.sub(
                            r'(<program[^>]*\blabel="devinfo"[^>]*filename=")[^"]*(".*/>)',
                            r'\1devinfo.img\2',
                            content,
                            flags=re.IGNORECASE
                        )
                        content = re.sub(
                            r'(<program[^>]*filename=")[^"]*("[^>]*\blabel="devinfo"[^>]*/>)',
                            r'\1devinfo.img\2',
                            content,
                            flags=re.IGNORECASE
                        )
                        
                        dest_devinfo_xml.write_text(content, encoding='utf-8')
                        print(get_string("act_created_devinfo_xml").format(name=dest_devinfo_xml.name, parent=dest_devinfo_xml.parent.name))
                    except Exception as e:
                        print(get_string("act_err_create_devinfo_xml").format(name=dest_devinfo_xml.name, e=e), file=sys.stderr)
                else:
                    print(get_string("act_warn_devinfo_xml_missing").format(name=src_devinfo_xml.name))

        except Exception as e:
            print(get_string("act_err_xml_mod").format(e=e), file=sys.stderr)
            raise
        
        print(get_string("act_clean_temp").format(dir=WORKING_DIR.name))
    
    print("\n" + "=" * 61)
    print(get_string("act_success"))
    print(get_string("act_xml_ready").format(dir=OUTPUT_XML_DIR.name))
    print(get_string("act_xml_next_step"))
    print("=" * 61)