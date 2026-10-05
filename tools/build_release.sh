#!/bin/sh
# Builds dist/gothic3_impexp-<version>.zip: the add-on with gothic3-core.exe inside (bin/),
# packed by Blender's own extension builder (validates the manifest).
#   BLENDER=/path/to/blender.exe tools/build_release.sh
set -e
cd "$(dirname "$0")/.."
BLENDER="${BLENDER:-blender}"
( cd core && cargo build --release )
# The collision cooker: a 32-bit helper that runs the game's own NxCooking.dll (built with the C# compiler of
# .NET Framework 4, present on every Windows).
( cd cook32 && /c/Windows/Microsoft.NET/Framework/v4.0.30319/csc.exe -nologo -platform:x86 -optimize -out:g3cook.exe g3cook.cs )
rm -rf dist/stage && mkdir -p dist/stage/bin
cp addon/gothic3_impexp/*.py addon/gothic3_impexp/blender_manifest.toml dist/stage/
cp core/target/release/gothic3-core.exe cook32/g3cook.exe dist/stage/bin/
cp LICENSE NOTICE dist/stage/
"$BLENDER" --background --factory-startup --command extension build --source-dir dist/stage --output-dir dist
rm -rf dist/stage
ls -l dist/*.zip
