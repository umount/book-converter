$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
if (-not [Environment]::Is64BitOperatingSystem) { throw "Windows x64 is required." }

# Build machine needs Node.js, Rust MSVC and Visual Studio C++ build tools.
# Testers only need the resulting installer; Python/Rust/Node are not required.
npm ci
if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }

& (Join-Path $PSScriptRoot "prepare-windows.ps1")
# The regular build hook builds the frontend.
npm run tauri -- build --debug --features diagnostics --target x86_64-pc-windows-msvc --bundles nsis
if ($LASTEXITCODE -ne 0) { throw "Diagnostic installer build failed" }
Write-Host "Installer: src-tauri/target/x86_64-pc-windows-msvc/debug/bundle/nsis/"
Write-Host "Distribute the installer, not the bare application .exe."
