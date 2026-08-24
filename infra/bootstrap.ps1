<#
.SYNOPSIS
One command from a fresh checkout to Leave Setup on Windows.

.DESCRIPTION
Installs the toolchains Leave needs into this user's account only, builds
Leave, installs it, and opens Leave Setup. Nothing here needs an
administrator, and everything is downloaded from the official Rust, Node.js,
and Leave sources.

.PARAMETER Yes
Install the missing prerequisites without asking.

.PARAMETER NoSetup
Install Leave but do not open Leave Setup afterwards.

.PARAMETER Prefix
Install Leave under this directory instead of %LOCALAPPDATA%\Leave.
#>
[CmdletBinding()]
param(
  [switch]$Yes,
  [switch]$NoSetup,
  [string]$Prefix = $env:LEAVE_INSTALL_PREFIX
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryDir = Split-Path -Parent $PSScriptRoot
if (-not $Prefix) { $Prefix = Join-Path $env:LOCALAPPDATA 'Leave' }
$toolchainDir = if ($env:LEAVE_TOOLCHAIN_DIR) { $env:LEAVE_TOOLCHAIN_DIR } else { Join-Path $env:LOCALAPPDATA 'Leave\toolchain' }
$nodeVersion = (Get-Content (Join-Path $repositoryDir '.nvmrc')).Trim()

function Write-Step([string]$Message) { Write-Host "`n==> $Message" -ForegroundColor Cyan }
function Stop-Bootstrap([string]$Message) { throw "Leave setup stopped: $Message" }

function Confirm-Step([string]$Question) {
  if ($Yes) { return $true }
  $answer = Read-Host "$Question [Y/n]"
  return ($answer -eq '' -or $answer -match '^(y|yes)$')
}

function Test-Command([string]$Name) {
  return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Test-NodeVersion {
  if (-not (Test-Command 'node')) { return $false }
  try { $version = [Version](node -p 'process.versions.node') } catch { return $false }
  return $version -ge [Version]'22.12.0'
}

function Install-Rust {
  Write-Step 'Installing Rust for your user account'
  if (-not (Confirm-Step 'Install the official Rust toolchain from https://win.rustup.rs?')) {
    Stop-Bootstrap 'Rust is required to build Leave'
  }
  $architecture = if ([Environment]::Is64BitOperatingSystem) { 'x86_64' } else { 'i686' }
  $installer = Join-Path $env:TEMP 'rustup-init.exe'
  Invoke-WebRequest -Uri "https://static.rust-lang.org/rustup/dist/$architecture-pc-windows-msvc/rustup-init.exe" -OutFile $installer -UseBasicParsing
  & $installer -y --profile minimal --no-modify-path | Out-Null
  Remove-Item $installer -Force
  $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
  Write-Host "Rust installed under $env:USERPROFILE\.cargo"
}

function Install-Node {
  Write-Step "Installing Node.js $nodeVersion for your user account"
  if (-not (Confirm-Step "Download Node.js $nodeVersion from https://nodejs.org?")) {
    Stop-Bootstrap "Node.js $nodeVersion or newer is required to build the Leave app"
  }
  $architecture = if ([Environment]::Is64BitOperatingSystem) { 'x64' } else { 'x86' }
  $archive = "node-v$nodeVersion-win-$architecture.zip"
  $download = Join-Path $env:TEMP $archive
  Invoke-WebRequest -Uri "https://nodejs.org/dist/v$nodeVersion/$archive" -OutFile $download -UseBasicParsing
  New-Item -ItemType Directory -Force -Path $toolchainDir | Out-Null
  $nodeDir = Join-Path $toolchainDir 'node'
  if (Test-Path $nodeDir) { Remove-Item $nodeDir -Recurse -Force }
  Expand-Archive -Path $download -DestinationPath $toolchainDir -Force
  Rename-Item (Join-Path $toolchainDir "node-v$nodeVersion-win-$architecture") 'node'
  Remove-Item $download -Force
  $env:PATH = "$nodeDir;$env:PATH"
  Write-Host "Node.js installed under $nodeDir"
}

Write-Step 'Checking this computer'
if (-not (Test-Path (Join-Path $repositoryDir 'Cargo.toml'))) {
  Stop-Bootstrap 'run this script from a Leave checkout'
}
if (-not (Test-Command 'git')) { Write-Host 'git is not installed. Leave''s Git features need it later.' }

if (Test-Command 'cargo') {
  Write-Host "Rust: found $(cargo --version)"
} elseif (Test-Path "$env:USERPROFILE\.cargo\bin\cargo.exe") {
  $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
  Write-Host "Rust: found $(cargo --version)"
} else {
  Write-Host 'Rust: not installed'
  Install-Rust
}

$bundledNode = Join-Path $toolchainDir 'node'
if ((Test-Path (Join-Path $bundledNode 'node.exe')) -and -not (Test-NodeVersion)) {
  $env:PATH = "$bundledNode;$env:PATH"
}

if (Test-NodeVersion) {
  Write-Host "Node.js: found $(node --version)"
} else {
  Write-Host "Node.js: not installed or older than v$nodeVersion"
  Install-Node
}

if (-not (Test-Command 'corepack')) { Stop-Bootstrap 'corepack is missing from this Node.js installation' }
corepack enable | Out-Null

Write-Step 'Building Leave (this takes a few minutes the first time)'
$env:LEAVE_INSTALL_PREFIX = $Prefix
& (Join-Path $PSScriptRoot 'install-local.ps1')

$binary = Join-Path $Prefix 'bin\leave.exe'
if (-not (Test-Path $binary)) { Stop-Bootstrap "the build finished but $binary is missing" }

Write-Step 'Leave is installed'
Write-Host 'Leave Setup guides you through Devin sign-in, choosing a folder, and phone access.'
Write-Host "You can reopen it any time from the Start menu, or run: $binary setup"

if (-not $NoSetup) {
  Write-Step 'Opening Leave Setup'
  & $binary setup
}
