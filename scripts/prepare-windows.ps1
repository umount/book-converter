$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$pdfium = Join-Path $PWD "src-tauri/pdfium"
New-Item -ItemType Directory -Force $pdfium | Out-Null
if (-not (Test-Path (Join-Path $pdfium "pdfium.dll"))) {
    $temp = Join-Path ([IO.Path]::GetTempPath()) ("bc-pdfium-" + [guid]::NewGuid())
    New-Item -ItemType Directory $temp | Out-Null
    try {
        Invoke-WebRequest "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.tgz" -OutFile (Join-Path $temp "pdfium.tgz")
        tar -xzf (Join-Path $temp "pdfium.tgz") -C $temp
        if ($LASTEXITCODE -ne 0) { throw "pdfium extraction failed" }
        Copy-Item (Join-Path $temp "bin/pdfium.dll") $pdfium
    } finally {
        Remove-Item -Recurse -Force $temp
    }
}
