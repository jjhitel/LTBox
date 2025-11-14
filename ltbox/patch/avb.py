import hashlib
import importlib.util
import shutil
import sys
from pathlib import Path
from typing import Dict, Any, List, Optional

from .. import constants as const
from .. import utils
from ..i18n import get_string

_avbtool_module = None

def _load_avbtool():
    global _avbtool_module
    if _avbtool_module:
        return _avbtool_module

    if not const.AVBTOOL_PY.exists():
        raise FileNotFoundError(f"avbtool not found at: {const.AVBTOOL_PY}")

    try:
        spec = importlib.util.spec_from_file_location("avbtool_lib", const.AVBTOOL_PY)
        if spec and spec.loader:
            module = importlib.util.module_from_spec(spec)
            sys.modules["avbtool_lib"] = module
            spec.loader.exec_module(module)
            _avbtool_module = module
            return module
    except Exception as e:
        raise RuntimeError(f"Failed to load avbtool: {e}")
    
    return _avbtool_module

def extract_image_avb_info(image_path: Path) -> Dict[str, Any]:
    image_path = Path(image_path)
    info: Dict[str, Any] = {}
    props_args: List[str] = []

    if not image_path.exists():
        return info
    
    info['partition_size'] = str(image_path.stat().st_size)

    try:
        avbtool = _load_avbtool()
        with open(image_path, 'rb') as f:
            image_data = f.read()
        
        avb = avbtool.Avb(image_data)

        if avb.footer:
            info['data_size'] = str(avb.footer.original_image_size)
            info['name'] = avb.footer.partition_name
        
        if avb.vbmeta_header:
            info['algorithm'] = avbtool.get_algorithm_name(avb.vbmeta_header.algorithm_type)
            info['rollback'] = str(avb.vbmeta_header.rollback_index)
            info['flags'] = str(avb.vbmeta_header.flags)
            
            if avb.vbmeta_header.public_key_size > 0:
                key_data = avb.aux_data[:avb.vbmeta_header.public_key_size]
                info['pubkey_sha1'] = hashlib.sha1(key_data).hexdigest()

        if avb.descriptors:
            for desc in avb.descriptors:
                if isinstance(desc, avbtool.AvbPropertyDescriptor):
                    info[desc.key] = desc.value
                    props_args.extend(["--prop", f"{desc.key}:{desc.value}"])
                elif isinstance(desc, avbtool.AvbHashDescriptor):
                    info['salt'] = desc.salt.hex()
                    if 'name' not in info:
                        info['name'] = desc.partition_name
                elif isinstance(desc, avbtool.AvbHashtreeDescriptor):
                    info['salt'] = desc.salt.hex()
                    if 'name' not in info:
                        info['name'] = desc.partition_name

    except Exception:
        pass

    info['props_args'] = props_args

    if 'flags' in info:
        print(get_string("img_info_flags").format(flags=info['flags']))
    
    if props_args:
        print(get_string("img_info_props").format(count=len(props_args) // 2))

    return info

def _run_avbtool_main(args: List[str]) -> None:
    avbtool = _load_avbtool()
    if not avbtool:
        raise RuntimeError("avbtool module not loaded")
    
    original_argv = sys.argv
    try:
        sys.argv = ["avbtool"] + args
        avbtool.main()
    finally:
        sys.argv = original_argv

def _apply_hash_footer(
    image_path: Path, 
    image_info: Dict[str, Any], 
    key_file: Path, 
    new_rollback_index: Optional[str] = None
) -> None:
    rollback_index = new_rollback_index if new_rollback_index is not None else image_info['rollback']
    
    print(get_string("img_footer_adding").format(name=image_path.name))
    print(get_string("img_footer_details").format(part=image_info['name'], rb=rollback_index))

    add_footer_cmd = [
        "add_hash_footer",
        "--image", str(image_path), 
        "--key", str(key_file),
        "--algorithm", image_info['algorithm'], 
        "--partition_size", image_info['partition_size'],
        "--partition_name", image_info['name'], 
        "--rollback_index", str(rollback_index),
        "--salt", image_info['salt'], 
        *image_info.get('props_args', [])
    ]
    
    if 'flags' in image_info:
        add_footer_cmd.extend(["--flags", image_info.get('flags', '0')])
        print(get_string("img_footer_restore_flags").format(flags=image_info.get('flags', '0')))

    _run_avbtool_main(add_footer_cmd)
    print(get_string("img_footer_success").format(name=image_path.name))

def patch_chained_image_rollback(
    image_name: str, 
    current_rb_index: int, 
    new_image_path: Path, 
    patched_image_path: Path
) -> None:
    try:
        print(get_string("img_analyze_new").format(name=image_name))
        info = extract_image_avb_info(new_image_path)
        new_rb_index = int(info.get('rollback', '0'))
        print(get_string("img_new_index").format(index=new_rb_index))

        if new_rb_index >= current_rb_index:
            print(get_string("img_index_ok").format(name=image_name))
            shutil.copy(new_image_path, patched_image_path)
            return

        print(get_string("img_patch_bypass").format(name=image_name, old=new_rb_index, new=current_rb_index))
        
        for key in ['partition_size', 'name', 'salt', 'algorithm', 'pubkey_sha1']:
            if key not in info:
                raise KeyError(get_string("img_err_missing_key").format(key=key, name=new_image_path.name))
        
        key_file = const.KEY_MAP.get(info['pubkey_sha1']) 
        if not key_file:
            raise KeyError(get_string("img_err_unknown_key").format(key=info['pubkey_sha1'], name=new_image_path.name))
        
        shutil.copy(new_image_path, patched_image_path)
        
        _apply_hash_footer(
            image_path=patched_image_path,
            image_info=info,
            key_file=key_file,
            new_rollback_index=str(current_rb_index)
        )

    except (KeyError, subprocess.CalledProcessError, FileNotFoundError) as e:
        print(get_string("img_err_processing").format(name=image_name, e=e), file=sys.stderr)
        raise

def patch_vbmeta_image_rollback(
    image_name: str, 
    current_rb_index: int, 
    new_image_path: Path, 
    patched_image_path: Path
) -> None:
    try:
        print(get_string("img_analyze_new").format(name=image_name))
        info = extract_image_avb_info(new_image_path)
        new_rb_index = int(info.get('rollback', '0'))
        print(get_string("img_new_index").format(index=new_rb_index))

        if new_rb_index >= current_rb_index:
            print(get_string("img_index_ok").format(name=image_name))
            shutil.copy(new_image_path, patched_image_path)
            return

        print(get_string("img_patch_bypass").format(name=image_name, old=new_rb_index, new=current_rb_index))

        for key in ['algorithm', 'pubkey_sha1']:
            if key not in info:
                raise KeyError(get_string("img_err_missing_key").format(key=key, name=new_image_path.name))
        
        key_file = const.KEY_MAP.get(info['pubkey_sha1']) 
        if not key_file:
            raise KeyError(get_string("img_err_unknown_key").format(key=info['pubkey_sha1'], name=new_image_path.name))

        remake_cmd = [
            "make_vbmeta_image",
            "--output", str(patched_image_path),
            "--key", str(key_file),
            "--algorithm", info['algorithm'],
            "--rollback_index", str(current_rb_index),
            "--flags", info.get('flags', '0'),
            "--include_descriptors_from_image", str(new_image_path)
        ]
        
        _run_avbtool_main(remake_cmd)
        print(get_string("img_patch_success").format(name=image_name))

    except (KeyError, subprocess.CalledProcessError, FileNotFoundError) as e:
        print(get_string("img_err_processing").format(name=image_name, e=e), file=sys.stderr)
        raise

def process_boot_image_avb(image_to_process: Path) -> None:
    print(get_string("img_verify_boot")) 
    boot_bak_img = const.BASE_DIR / "boot.bak.img"
    if not boot_bak_img.exists():
        print(get_string("img_err_boot_bak_missing").format(name=boot_bak_img.name), file=sys.stderr)
        raise FileNotFoundError(f"{boot_bak_img.name} not found.")
        
    boot_info = extract_image_avb_info(boot_bak_img)
    
    for key in ['partition_size', 'name', 'rollback', 'salt', 'algorithm', 'pubkey_sha1']:
        if key not in boot_info:
            raise KeyError(get_string("img_err_missing_key").format(key=key, name=boot_bak_img.name))
            
    boot_pubkey = boot_info.get('pubkey_sha1')
    key_file = const.KEY_MAP.get(boot_pubkey) 
    
    if not key_file:
        print(get_string("img_err_boot_key_mismatch").format(key=boot_pubkey))
        raise KeyError(f"Unknown boot public key: {boot_pubkey}")

    print(get_string("img_key_matched").format(name=key_file.name))
    
    _apply_hash_footer(
        image_path=image_to_process,
        image_info=boot_info,
        key_file=key_file
    )