# Thocci automated installer for Windows PowerShell (CLI + Desktop App).
# Keep this file ASCII-only for Windows PowerShell 5.1 compatibility.
param(
    [switch]$CliOnly
)

$ErrorActionPreference = "Stop"

$Repo = "ItzEvhyx/Thocci"
$FallbackVersion = "v0.1.0"
$InstallDir = "$env:USERPROFILE\.local\bin"
$ExePath = "$InstallDir\thocci.exe"
$DesktopExePath = "$InstallDir\thocci-app.exe"
$AssetTarget = "x86_64-pc-windows-msvc"

if ($env:THOCCI_CLI_ONLY -eq "1") {
    $CliOnly = $true
}

Write-Host "Installing Thocci for Windows..." -ForegroundColor Cyan

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$ReleaseJson = $null
function Get-LatestRelease {
    if ($null -eq $script:ReleaseJson) {
        try {
            $script:ReleaseJson = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "thocci-installer" }
        } catch {
            $script:ReleaseJson = $false
        }
    }
    return $script:ReleaseJson
}

function Verify-DownloadSha256 {
    param([string]$FilePath, [string]$ShaUrl)
    if (-not $ShaUrl) { return }
    try {
        $ShaText = (Invoke-RestMethod -Uri $ShaUrl -Headers @{ "User-Agent" = "thocci-installer" }).ToString().Trim()
        $Expected = ($ShaText -split '\s+')[0].ToLowerInvariant()
        $Actual = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($Expected -and ($Expected -ne $Actual)) {
            throw "Checksum mismatch! Expected $Expected, got $Actual."
        }
    } catch {
        if ($_.Exception.Message -like "Checksum mismatch*") {
            throw
        }
    }
}

function Create-StartMenuShortcut {
    param([string]$TargetExe)
    try {
        $StartMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
        if (Test-Path $StartMenu) {
            $ShortcutPath = Join-Path $StartMenu "Thocci.lnk"
            $Shell = New-Object -ComObject WScript.Shell
            $Shortcut = $Shell.CreateShortcut($ShortcutPath)
            $Shortcut.TargetPath = $TargetExe
            $Shortcut.WorkingDirectory = Split-Path -Parent $TargetExe
            $Shortcut.IconLocation = "$TargetExe,0"
            $Shortcut.Description = "Thocci - Mechanical keyboard sounds"
            $Shortcut.Save()
        }
    } catch {
        # Shortcut creation is optional; do not fail installation.
    }
}

# 1. Install CLI binary (always preserved)
# $PSScriptRoot is empty when piped into iex (irm | iex)
$ScriptDir = $PSScriptRoot
if ($ScriptDir -and (Test-Path "$ScriptDir\target\release\thocci.exe")) {
    Copy-Item "$ScriptDir\target\release\thocci.exe" $ExePath -Force
} elseif ($ScriptDir -and (Test-Path "$ScriptDir\thocci.exe")) {
    Copy-Item "$ScriptDir\thocci.exe" $ExePath -Force
} else {
    $ZipUrl = ""
    $ShaUrl = ""
    $Rel = Get-LatestRelease
    if ($Rel -and $Rel.assets) {
        $Asset = $Rel.assets | Where-Object { $_.name -like "thocci-*$AssetTarget*.zip" -and $_.name -notlike "thocci-desktop-*" -and $_.name -notlike "*.sha256" } | Select-Object -First 1
        if ($Asset) { $ZipUrl = $Asset.browser_download_url }
        $ShaAsset = $Rel.assets | Where-Object { $_.name -like "thocci-*$AssetTarget*.zip.sha256" -and $_.name -notlike "thocci-desktop-*" } | Select-Object -First 1
        if ($ShaAsset) { $ShaUrl = $ShaAsset.browser_download_url }
    }

    if (-not $ZipUrl) {
        $ZipUrl = "https://github.com/$Repo/releases/download/$FallbackVersion/thocci-$FallbackVersion-$AssetTarget.zip"
        $ShaUrl = "$ZipUrl.sha256"
    }

    $TempZip = Join-Path $env:TEMP "thocci.zip"
    $TempExtract = Join-Path $env:TEMP "thocci_extract"

    Write-Host "Downloading $ZipUrl..." -ForegroundColor Gray
    Invoke-WebRequest -Uri $ZipUrl -OutFile $TempZip
    Verify-DownloadSha256 -FilePath $TempZip -ShaUrl $ShaUrl

    if (Test-Path $TempExtract) {
        Remove-Item $TempExtract -Recurse -Force -ErrorAction SilentlyContinue
    }
    Expand-Archive -Path $TempZip -DestinationPath $TempExtract -Force
    Copy-Item (Join-Path $TempExtract "thocci.exe") $ExePath -Force
    Remove-Item $TempZip -Force -ErrorAction SilentlyContinue
    Remove-Item $TempExtract -Recurse -Force -ErrorAction SilentlyContinue
}

# 2. Install Desktop GUI binary alongside CLI when not -CliOnly
$DesktopInstalled = $false
if (-not $CliOnly) {
    if ($ScriptDir -and (Test-Path "$ScriptDir\target\release\thocci-app.exe")) {
        Copy-Item "$ScriptDir\target\release\thocci-app.exe" $DesktopExePath -Force
        $DesktopInstalled = $true
    } elseif ($ScriptDir -and (Test-Path "$ScriptDir\thocci-app.exe")) {
        Copy-Item "$ScriptDir\thocci-app.exe" $DesktopExePath -Force
        $DesktopInstalled = $true
    } else {
        $DesktopZipUrl = ""
        $DesktopShaUrl = ""
        $Rel = Get-LatestRelease
        if ($Rel -and $Rel.assets) {
            $DAsset = $Rel.assets | Where-Object { $_.name -like "thocci-desktop-*$AssetTarget*.zip" -and $_.name -notlike "*.sha256" } | Select-Object -First 1
            if ($DAsset) { $DesktopZipUrl = $DAsset.browser_download_url }
            $DSha = $Rel.assets | Where-Object { $_.name -like "thocci-desktop-*$AssetTarget*.zip.sha256" } | Select-Object -First 1
            if ($DSha) { $DesktopShaUrl = $DSha.browser_download_url }
        }
        if (-not $DesktopZipUrl) {
            $DesktopZipUrl = "https://github.com/$Repo/releases/download/$FallbackVersion/thocci-desktop-$FallbackVersion-$AssetTarget.zip"
            $DesktopShaUrl = "$DesktopZipUrl.sha256"
        }

        $TempDesktopZip = Join-Path $env:TEMP "thocci-desktop.zip"
        $TempDesktopExtract = Join-Path $env:TEMP "thocci_desktop_extract"
        try {
            Write-Host "Downloading $DesktopZipUrl..." -ForegroundColor Gray
            Invoke-WebRequest -Uri $DesktopZipUrl -OutFile $TempDesktopZip
            Verify-DownloadSha256 -FilePath $TempDesktopZip -ShaUrl $DesktopShaUrl
            if (Test-Path $TempDesktopExtract) {
                Remove-Item $TempDesktopExtract -Recurse -Force -ErrorAction SilentlyContinue
            }
            Expand-Archive -Path $TempDesktopZip -DestinationPath $TempDesktopExtract -Force
            if (Test-Path (Join-Path $TempDesktopExtract "thocci-app.exe")) {
                Copy-Item (Join-Path $TempDesktopExtract "thocci-app.exe") $DesktopExePath -Force
                $DesktopInstalled = $true
            }
        } catch {
            if ($_.Exception.Message -like "Checksum mismatch*") {
                throw
            }
            Write-Host "Desktop bundle unavailable; continuing with CLI installation." -ForegroundColor Yellow
        } finally {
            Remove-Item $TempDesktopZip -Force -ErrorAction SilentlyContinue
            Remove-Item $TempDesktopExtract -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    if ($DesktopInstalled) {
        Create-StartMenuShortcut -TargetExe $DesktopExePath
    }
}

# Add ~/.local/bin to User PATH if missing
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not $UserPath) {
    $UserPath = ""
}
$PathEntries = $UserPath -split ';' | Where-Object { $_ -ne "" }
if ($PathEntries -notcontains $InstallDir) {
    Write-Host "Adding $InstallDir to User PATH..." -ForegroundColor Yellow
    $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path += ";$InstallDir"
}

Write-Host ""
$Check = [char]0x2713
Write-Host "$Check thocci CLI was successfully installed to $ExePath!" -ForegroundColor Green
if ($DesktopInstalled) {
    Write-Host "$Check Thocci Desktop app was installed to $DesktopExePath!" -ForegroundColor Green
}
Write-Host ""

if ($DesktopInstalled -and (-not $CliOnly) -and [Environment]::UserInteractive) {
    try {
        Write-Host "Launching Thocci..." -ForegroundColor Cyan
        Start-Process -FilePath $DesktopExePath
        exit 0
    } catch {
        # Fall back to CLI welcome if graphical launch fails.
    }
}

try {
    & "$ExePath" --welcome
} catch {
    Write-Host "Run 'thocci on' to start mechanical keyboard sounds."
}
