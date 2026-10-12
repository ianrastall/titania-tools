# Builds the native core (core/ -> titania_core.dll) with Cargo and copies it to artifacts\core.
# Called automatically by the app projects; run it directly with -Test to also run the unit tests.
param(
    [switch] $Test
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$manifest = Join-Path $root 'core\Cargo.toml'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo was not found. Install Rust from https://rustup.rs'
}

if ($Test) {
    cargo test --release --manifest-path $manifest | Out-Host
    if ($LASTEXITCODE) { throw 'Native core tests failed.' }
}

# Always a release build: an unoptimised core reads PGN many times slower.
cargo build --release --manifest-path $manifest -p titania-ffi | Out-Host
if ($LASTEXITCODE) { throw 'Native core build failed.' }

# The target directory may be redirected by CARGO_TARGET_DIR or a Cargo config, so ask Cargo where it is.
$target = (cargo metadata --manifest-path $manifest --no-deps --format-version 1 | ConvertFrom-Json).target_directory
$outDir = Join-Path $root 'artifacts\core'
New-Item -ItemType Directory -Force $outDir | Out-Null
$dll = Join-Path $outDir 'titania_core.dll'
Copy-Item (Join-Path $target 'release\titania_core.dll') $dll -Force
# Stamp the copy so MSBuild sees it as newer than the sources even when Cargo had nothing to rebuild.
(Get-Item $dll).LastWriteTime = Get-Date
