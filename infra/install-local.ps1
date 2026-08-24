$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Repository = Split-Path -Parent $PSScriptRoot
$InstallPrefix = if ($env:LEAVE_INSTALL_PREFIX) {
    $env:LEAVE_INSTALL_PREFIX
} else {
    Join-Path $env:LOCALAPPDATA "Leave"
}
$BinaryDirectory = Join-Path $InstallPrefix "bin"
$WebDirectory = Join-Path $InstallPrefix "share\leave\web"

Push-Location $Repository
try {
    corepack pnpm install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw "pnpm install failed" }
    corepack pnpm --filter '@leave/web' build
    if ($LASTEXITCODE -ne 0) { throw "PWA build failed" }
    cargo build --release -p leave
    if ($LASTEXITCODE -ne 0) { throw "Rust build failed" }

    New-Item -ItemType Directory -Force -Path $BinaryDirectory, $WebDirectory | Out-Null
    Copy-Item -Force (Join-Path $Repository "target\release\leave.exe") (Join-Path $BinaryDirectory "leave.exe")
    Copy-Item -Recurse -Force (Join-Path $Repository "apps\web\dist\*") $WebDirectory
} finally {
    Pop-Location
}

$StartMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
New-Item -ItemType Directory -Force -Path $StartMenu | Out-Null
$ShortcutPath = Join-Path $StartMenu "Leave Setup.lnk"
$Shell = New-Object -ComObject WScript.Shell
$Shortcut = $Shell.CreateShortcut($ShortcutPath)
$LeaveExecutable = Join-Path $BinaryDirectory "leave.exe"
$EscapedLeaveExecutable = $LeaveExecutable.Replace("'", "''")
$Shortcut.TargetPath = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
$Shortcut.Arguments = "-NoProfile -WindowStyle Hidden -Command `"& '$EscapedLeaveExecutable' setup`""
$Shortcut.WorkingDirectory = $InstallPrefix
$Shortcut.IconLocation = "$LeaveExecutable,0"
$Shortcut.Description = "Connect Devin and private phone access"
$Shortcut.Save()

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
$PathEntries = @($UserPath -split ';' | Where-Object { $_ })
if ($PathEntries -notcontains $BinaryDirectory) {
    [Environment]::SetEnvironmentVariable("Path", (($PathEntries + $BinaryDirectory) -join ';'), "User")
    Write-Host "Added $BinaryDirectory to your user PATH. Open a new terminal before running leave."
}

Write-Host "Leave installed locally at $BinaryDirectory\leave.exe"
Write-Host "Open Leave Setup from the Start menu."
Write-Host "Command-line fallback: leave setup"
