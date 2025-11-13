import re
import shutil
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Optional, List, Dict, Any

from .constants import *
from . import utils
from .crypto import decrypt_file
from .i18n import get_string

def scan_and_decrypt_xmls() -> List[Path]:
    OUTPUT_XML_DIR.mkdir(exist_ok=True)
    
    xmls = list(OUTPUT_XML_DIR.glob("rawprogram*.xml"))
    if not xmls:
        xmls = list(IMAGE_DIR.glob("rawprogram*.xml"))
    
    if not xmls:
        print(get_string("act_xml_scan_x"))
        x_files = list(IMAGE_DIR.glob("*.x"))
        
        if x_files:
            print(get_string("act_xml_found_x_count").format(len=len(x_files)))
            utils.check_dependencies() 
            for x_file in x_files:
                xml_name = x_file.stem + ".xml"
                out_path = OUTPUT_XML_DIR / xml_name
                if not out_path.exists():
                    print(get_string("act_xml_decrypting").format(name=x_file.name))
                    if decrypt_file(str(x_file), str(out_path)):
                        xmls.append(out_path)
                    else:
                        print(get_string("act_xml_decrypt_fail").format(name=x_file.name))
        else:
            print(get_string("act_xml_none_found"))
            print(get_string("act_xml_dump_req"))
            print(get_string("act_xml_place_prompt"))
            return []
            
    return xmls

def get_partition_params(target_label: str, xml_paths: List[Path]) -> Optional[Dict[str, Any]]:
    for xml_path in xml_paths:
        try:
            tree = ET.parse(xml_path)
            root = tree.getroot()
            for prog in root.findall('program'):
                label = prog.get('label', '').lower()
                if label == target_label.lower():
                    return {
                        'lun': prog.get('physical_partition_number'),
                        'start_sector': prog.get('start_sector'),
                        'num_sectors': prog.get('num_partition_sectors'),
                        'filename': prog.get('filename', ''),
                        'source_xml': xml_path.name
                    }
        except Exception as e:
            print(get_string("act_xml_parse_err").format(name=xml_path.name, e=e))
            
    return None

def ensure_params_or_fail(label: str) -> Dict[str, Any]:
    xmls = scan_and_decrypt_xmls()
    if not xmls:
        raise FileNotFoundError(get_string("act_err_no_xml_dump"))
        
    params = get_partition_params(label, xmls)
    if not params:
        if label == "boot":
            params = get_partition_params("boot_a", xmls)
            if not params:
                 params = get_partition_params("boot_b", xmls)
                 
    if not params:
        print(get_string("act_err_part_info_missing").format(label=label))
        raise ValueError(get_string("act_err_part_not_found").format(label=label))
        
    return params