# /// script
# requires-python = ">=3.10"
# dependencies = ["cryptography>=42"]
# ///
"""Encrypt / decrypt ~/.fund-rs/holdings.json so it can be synced as a text file.

Usage:
    uv run scripts/holdings_crypt.py encrypt   # ~/.fund-rs/holdings.json -> ./holdings.encode.txt
    uv run scripts/holdings_crypt.py decrypt   # ./holdings.encode.txt -> ~/.fund-rs/holdings.json

Format of holdings.encode.txt: "v1:" + base64(salt[16] | nonce[12] | AES-256-GCM ciphertext).
Key is derived from the terminal password with scrypt, so a wrong password fails the GCM
tag check instead of silently producing garbage.
"""

import argparse
import base64
import datetime as dt
import getpass
import json
import os
import sys
from pathlib import Path

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.scrypt import Scrypt

HOLDINGS_PATH = Path.home() / ".fund-rs" / "holdings.json"
ENCODED_PATH = Path.cwd() / "holdings.encode.txt"

VERSION_PREFIX = "v1:"
SALT_LEN = 16
NONCE_LEN = 12
# scrypt cost: n=2**17 is ~128 MiB / well under a second on a laptop; brute force stays expensive.
SCRYPT_N, SCRYPT_R, SCRYPT_P = 2**17, 8, 1
MAX_BACKUP_SUFFIX = 100


def derive_key(password: str, salt: bytes) -> bytes:
    kdf = Scrypt(salt=salt, length=32, n=SCRYPT_N, r=SCRYPT_R, p=SCRYPT_P)
    return kdf.derive(password.encode("utf-8"))


def read_password(confirm: bool) -> str:
    password = getpass.getpass("密钥: ")
    if not password:
        sys.exit("错误: 密钥不能为空")
    if confirm and getpass.getpass("再次输入密钥: ") != password:
        sys.exit("错误: 两次输入的密钥不一致")
    return password


def encrypt() -> None:
    if not HOLDINGS_PATH.is_file():
        sys.exit(f"错误: 找不到 {HOLDINGS_PATH}")
    plaintext = HOLDINGS_PATH.read_bytes()
    # Refuse to encrypt a broken file; otherwise the bad copy would propagate on decrypt.
    try:
        json.loads(plaintext)
    except json.JSONDecodeError as e:
        sys.exit(f"错误: {HOLDINGS_PATH} 不是合法 JSON: {e}")

    password = read_password(confirm=True)
    salt = os.urandom(SALT_LEN)
    nonce = os.urandom(NONCE_LEN)
    ciphertext = AESGCM(derive_key(password, salt)).encrypt(nonce, plaintext, None)
    payload = base64.b64encode(salt + nonce + ciphertext).decode("ascii")

    ENCODED_PATH.write_text(VERSION_PREFIX + payload + "\n", encoding="ascii")
    print(f"已加密: {HOLDINGS_PATH} -> {ENCODED_PATH}")


def backup_path_for(target: Path) -> Path:
    stamp = dt.date.today().strftime("%Y%m%d")
    candidate = target.with_name(f"{target.name}.{stamp}.bak")
    # Never overwrite an earlier backup from the same day.
    for i in range(1, MAX_BACKUP_SUFFIX + 1):
        if not candidate.exists():
            return candidate
        candidate = target.with_name(f"{target.name}.{stamp}.{i}.bak")
    sys.exit(f"错误: 当天备份文件过多（>{MAX_BACKUP_SUFFIX}），请先清理 {target.parent}")


def decrypt() -> None:
    if not ENCODED_PATH.is_file():
        sys.exit(f"错误: 找不到 {ENCODED_PATH}")
    text = ENCODED_PATH.read_text(encoding="ascii").strip()
    if not text.startswith(VERSION_PREFIX):
        sys.exit(f"错误: {ENCODED_PATH} 格式不识别（缺少 {VERSION_PREFIX} 前缀）")
    try:
        raw = base64.b64decode(text[len(VERSION_PREFIX):], validate=True)
    except ValueError as e:
        sys.exit(f"错误: base64 解码失败: {e}")
    if len(raw) <= SALT_LEN + NONCE_LEN:
        sys.exit("错误: 密文长度不足，文件已损坏")
    salt, nonce, ciphertext = raw[:SALT_LEN], raw[SALT_LEN:SALT_LEN + NONCE_LEN], raw[SALT_LEN + NONCE_LEN:]

    password = read_password(confirm=False)
    try:
        plaintext = AESGCM(derive_key(password, salt)).decrypt(nonce, ciphertext, None)
    except InvalidTag:
        sys.exit("错误: 解密失败（密钥错误或文件被篡改）")
    try:
        json.loads(plaintext)
    except json.JSONDecodeError as e:
        sys.exit(f"错误: 解密结果不是合法 JSON，未写入: {e}")

    HOLDINGS_PATH.parent.mkdir(parents=True, exist_ok=True)
    if HOLDINGS_PATH.exists():
        backup = backup_path_for(HOLDINGS_PATH)
        HOLDINGS_PATH.rename(backup)
        print(f"已备份原文件: {backup}")

    # Write to a temp file then rename, so an interrupted write never leaves a half file.
    tmp = HOLDINGS_PATH.with_name(HOLDINGS_PATH.name + ".tmp")
    tmp.write_bytes(plaintext)
    tmp.replace(HOLDINGS_PATH)
    print(f"已解密: {ENCODED_PATH} -> {HOLDINGS_PATH}")


def main() -> None:
    parser = argparse.ArgumentParser(description="加解密 holdings.json")
    parser.add_argument("action", choices=["encrypt", "decrypt"])
    args = parser.parse_args()
    encrypt() if args.action == "encrypt" else decrypt()


if __name__ == "__main__":
    main()
