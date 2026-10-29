<#
  Builds ApolloLauncher-<version>.msi from the current source.

  Usage (PowerShell, from this folder):
      .\make-installer.ps1              # build the release exe, then the MSI
      .\make-installer.ps1 -SkipBuild   # reuse target\release\apollolauncher.exe

  The first run downloads the WiX Toolset 3.14 binaries (a zip, no install, no admin) into
  installer\.wix. The MSI installs per user into %LOCALAPPDATA%\Programs\ApolloLauncher, so the
  app can write its portable "data" folder next to the exe without admin rights.
#>
param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$src = $root
$installer = Join-Path $src 'installer'
$wixDir = Join-Path $installer '.wix'
$exe = Join-Path $src 'target\release\apollolauncher.exe'
$icon = Join-Path $src 'assets\icon.ico'

$match = Select-String -Path (Join-Path $src 'Cargo.toml') -Pattern '^version\s*=\s*"(\d+\.\d+\.\d+)"' |
    Select-Object -First 1
if (-not $match) { throw 'Could not read the version from Cargo.toml.' }
$version = $match.Matches.Groups[1].Value

if (-not $SkipBuild) {
    # Optional: set LLVM_MINGW_BIN to the llvm-mingw bin folder if it is not already on PATH.
    if ($env:LLVM_MINGW_BIN -and (Test-Path $env:LLVM_MINGW_BIN)) { $env:PATH = "$($env:LLVM_MINGW_BIN);$env:PATH" }
    Write-Host 'Building release exe...'
    Push-Location $src
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
    }
    finally { Pop-Location }
}
if (-not (Test-Path $exe)) { throw "Missing $exe. Run without -SkipBuild." }

if (-not (Test-Path (Join-Path $wixDir 'candle.exe'))) {
    Write-Host 'Downloading WiX Toolset 3.14 binaries...'
    New-Item -ItemType Directory -Force $wixDir | Out-Null
    $zip = Join-Path $wixDir 'wix314-binaries.zip'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -UseBasicParsing -OutFile $zip `
        -Uri 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip'
    Expand-Archive -Path $zip -DestinationPath $wixDir -Force
    Remove-Item $zip
}

$obj = Join-Path $installer 'ApolloLauncher.wixobj'
$msi = Join-Path $root "ApolloLauncher-$version.msi"
Write-Host "Compiling installer $version..."
& (Join-Path $wixDir 'candle.exe') -nologo -arch x64 "-dVersion=$version" "-dExeFile=$exe" `
    "-dIconFile=$icon" -out $obj (Join-Path $installer 'ApolloLauncher.wxs')
if ($LASTEXITCODE -ne 0) { throw 'candle failed.' }
& (Join-Path $wixDir 'light.exe') -nologo -out $msi $obj
if ($LASTEXITCODE -ne 0) { throw 'light failed.' }
Remove-Item $obj, ([IO.Path]::ChangeExtension($msi, '.wixpdb')) -ErrorAction SilentlyContinue

Write-Host "Done: $msi"

