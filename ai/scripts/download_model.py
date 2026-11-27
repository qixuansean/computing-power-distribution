import os
import hashlib
import json
import sys
from huggingface_hub import hf_hub_download

# 模型配置与清单路径
MANIFEST_FILE = "ai/manifests/model_checksums.json"
MODEL_ID = "ibm-granite/granite-timeseries-ttm-r2"
REVISION = "main"
FILENAME = "model.safetensors"
TARGET_DIR = "ai/models/granite-ttm/model"

def verify_checksum(filepath, expected_sha256):
    """验证文件的 SHA256 校验和"""
    sha256 = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(8192):
            sha256.update(chunk)
    return sha256.hexdigest() == expected_sha256

def download_model():
    """执行模型下载与校验流程"""
    # 强制设置输出编码为 UTF-8 以避免 Windows 编码问题
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8')

    print(f"--- OCN AI Model Download Stage ---")
    print(f"Target Model: {MODEL_ID}")
    print(f"Revision: {REVISION}")
    
    # 确保目录存在
    os.makedirs(TARGET_DIR, exist_ok=True)
    temp_dir = os.path.join("ai", "temp")
    os.makedirs(temp_dir, exist_ok=True)
    
    # 从清单读取预期校验值
    try:
        with open(MANIFEST_FILE, 'r') as f:
            manifest = json.load(f)
            expected_info = manifest.get("granite-timeseries-ttm-r2")
            if not expected_info:
                print(f"[ERROR] Model 'granite-timeseries-ttm-r2' not found in manifest.")
                sys.exit(1)
            expected_sha256 = expected_info["sha256"]
            expected_size = expected_info.get("size_bytes")
    except Exception as e:
        print(f"[ERROR] Reading manifest: {e}")
        sys.exit(1)

    print(f"Expected SHA256: {expected_sha256}")
    
    try:
        print(f"Downloading {FILENAME} from HuggingFace...")
        downloaded_path = hf_hub_download(
            repo_id=MODEL_ID,
            filename=FILENAME,
            revision=REVISION,
            local_dir=temp_dir
        )
        
        # 校验下载的文件
        actual_size = os.path.getsize(downloaded_path)
        print(f"Download complete. Size: {actual_size} bytes")
        
        if expected_size and actual_size != expected_size:
            print(f"[WARNING] File size mismatch. Expected {expected_size}, got {actual_size}")
            
        print("Verifying SHA256...")
        if verify_checksum(downloaded_path, expected_sha256):
            print("[OK] Checksum verified successfully.")
            
            # 原子替换
            final_path = os.path.join(TARGET_DIR, FILENAME)
            if os.path.exists(final_path):
                os.remove(final_path)
            os.rename(downloaded_path, final_path)
            
            print(f"Model deployed to: {final_path}")
            
            # 输出最终状态
            print(f"\nSummary:")
            print(f"- Version: {MODEL_ID}@{REVISION}")
            print(f"- Status: Verified & Deployed")
            print(f"- SHA256: {expected_sha256}")
            print(f"- Format: Safetensors")
        else:
            print("[ERROR] SHA256 verification failed! Deleting corrupted file.")
            os.remove(downloaded_path)
            sys.exit(1)
            
    except Exception as e:
        print(f"[ERROR] Download or deployment failed: {e}")
        sys.exit(1)

if __name__ == "__main__":
    download_model()
