"""Скрипт автоматической упаковки мода Vibe Physics в формат .hnmod для лаунчера Horizon и публикации в реестре playhorizon.dev."""
import hashlib
import json
import os
import shutil
import zipfile

MOD_ID = "vibe_physics"
VERSION = "1.0.0"
OUTPUT_FILE = "vibe_physics.hnmod"
OUTPUT_VERSIONED = f"{MOD_ID}-{VERSION}.hnmod"

# Точные ревизии коммитов клиента Horizon 26.3 и horizon-mixins из установленного инстанса
HORIZON_REVISION = "6475f62f3c834b7489e1816bf47f47ea5eb2b879"
MIXINS_REVISION = "d9c83d7096b7ea5b83678da2efdfd6161f74997e"

MANIFEST = {
    "game": "26.3",
    "hooks": 1,
    "horizon": HORIZON_REVISION,
    "id": MOD_ID,
    "mixins": MIXINS_REVISION,
    "schema": 1,
    "version": VERSION,
}

def compute_sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()

def package_mod():
    print(f"[*] Сборка пакета {OUTPUT_VERSIONED} ({OUTPUT_FILE})...")
    mixins_source = "mixins_pkg"
    if not os.path.exists(mixins_source):
        raise RuntimeError(f"Папка {mixins_source} не найдена.")

    for out in [OUTPUT_FILE, OUTPUT_VERSIONED]:
        with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as z:
            # 1. Корневой манифест нативного мода
            z.writestr("horizon-native.json", json.dumps(MANIFEST, indent=2))
            
            # 2. Исходный код миксинов мода
            for file in sorted(os.listdir(mixins_source)):
                if file.endswith((".rs", ".toml")):
                    full_path = os.path.join(mixins_source, file)
                    zip_target = f"mixins/{MOD_ID}/{file}"
                    z.write(full_path, zip_target)

    size_bytes = os.path.getsize(OUTPUT_VERSIONED)
    size_kb = size_bytes / 1024
    sha256_hash = compute_sha256(OUTPUT_VERSIONED)

    print(f"[+] Готово! Файлы созданы:")
    print(f"    - {OUTPUT_VERSIONED} ({size_bytes} байт, {size_kb:.1f} КБ)")
    print(f"    - {OUTPUT_FILE} ({size_bytes} байт, {size_kb:.1f} КБ)")
    print(f"    - SHA-256: {sha256_hash}")

    # 3. Автоматическое копирование в папку модов Horizon
    dest_dir = os.path.expandvars(r"%APPDATA%\horizon\horizon-native-mods")
    if os.path.exists(dest_dir):
        shutil.copy2(OUTPUT_FILE, os.path.join(dest_dir, OUTPUT_FILE))
        shutil.copy2(OUTPUT_VERSIONED, os.path.join(dest_dir, OUTPUT_VERSIONED))
        print(f"[+] Файлы успешно скопированы в {dest_dir}")

    # 4. Обновление в installed.incoming (лаунчер)
    incoming_dir = os.path.expandvars(r"%APPDATA%\horizon-launcher\native\installed.incoming\vibe_physics")
    if os.path.exists(incoming_dir):
        shutil.rmtree(incoming_dir)
    os.makedirs(os.path.join(incoming_dir, "mixins", MOD_ID), exist_ok=True)
    with open(os.path.join(incoming_dir, "horizon-native.json"), "w", encoding="utf-8") as f:
        json.dump(MANIFEST, f, indent=2)
    for file in os.listdir(mixins_source):
        if file.endswith((".rs", ".toml")):
            shutil.copy2(os.path.join(mixins_source, file), os.path.join(incoming_dir, "mixins", MOD_ID, file))
    print(f"[+] Обновлена папка staged мода в лаунчере: {incoming_dir}")

if __name__ == "__main__":
    package_mod()
