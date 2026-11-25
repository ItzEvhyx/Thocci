# Thocci automated installer for Windows PowerShell.
# Keep this file ASCII-only for Windows PowerShell 5.1 compatibility.
$ErrorActionPreference = "Stop"

$Repo = "ItzEvhyx/Thocci"
$FallbackVersion = "v0.1.0"
$InstallDir = "$env:USERPROFILE\.local\bin"
$ExePath = "$InstallDir\thocci.exe"

Write-Host "Installing Thocci for Windows..." -ForegroundColor Cyan

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# $PSScriptRoot is empty when piped into iex (irm | iex)
$ScriptDir = $PSScriptRoot
if ($ScriptDir -and (Test-Path "$ScriptDir\target\release\thocci.exe")) {
    Copy-Item "$ScriptDir\target\release\thocci.exe" $ExePath -Force
} elseif ($ScriptDir -and (Test-Path "$ScriptDir\thocci.exe")) {
    Copy-Item "$ScriptDir\thocci.exe" $ExePath -Force
} else {
    $AssetTarget = "x86_64-pc-windows-msvc"
    $ZipUrl = ""
    $ShaUrl = ""
    try {
        $ReleaseJson = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "thocci-installer" }
        $Asset = $ReleaseJson.assets | Where-Object { $_.name -like "*$AssetTarget*.zip" -and $_.name -notlike "*.sha256" } | Select-Object -First 1
        if ($Asset) {
            $ZipUrl = $Asset.browser_download_url
        }
        $ShaAsset = $ReleaseJson.assets | Where-Object { $_.name -like "*$AssetTarget*.zip.sha256" } | Select-Object -First 1
        if ($ShaAsset) {
            $ShaUrl = $ShaAsset.browser_download_url
        }
    } catch {
        # Ignore API errors / rate limits and fall back to tagged release URL.
    }

    if (-not $ZipUrl) {
        $ZipUrl = "https://github.com/$Repo/releases/download/$FallbackVersion/thocci-$FallbackVersion-$AssetTarget.zip"
        $ShaUrl = "$ZipUrl.sha256"
    }

    $TempZip = Join-Path $env:TEMP "thocci.zip"
    $TempExtract = Join-Path $env:TEMP "thocci_extract"

    Write-Host "Downloading $ZipUrl..." -ForegroundColor Gray
    Invoke-WebRequest -Uri $ZipUrl -OutFile $TempZip

    if ($ShaUrl) {
        try {
            $ShaText = (Invoke-RestMethod -Uri $ShaUrl -Headers @{ "User-Agent" = "thocci-installer" }).ToString().Trim()
            $Expected = ($ShaText -split '\s+')[0].ToLowerInvariant()
            $Actual = (Get-FileHash -Path $TempZip -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($Expected -and ($Expected -ne $Actual)) {
                throw "Checksum mismatch! Expected $Expected, got $Actual."
            }
        } catch {
            if ($_.Exception.Message -like "Checksum mismatch*") {
                throw
            }
        }
    }

    if (Test-Path $TempExtract) {
        Remove-Item $TempExtract -Recurse -Force -ErrorAction SilentlyContinue
    }
    Expand-Archive -Path $TempZip -DestinationPath $TempExtract -Force
    Copy-Item (Join-Path $TempExtract "thocci.exe") $ExePath -Force
    Remove-Item $TempZip -Force -ErrorAction SilentlyContinue
    Remove-Item $TempExtract -Recurse -Force -ErrorAction SilentlyContinue
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
Write-Host "$Check thocci was successfully installed to $ExePath!" -ForegroundColor Green
Write-Host ""

try {
    & "$ExePath" --welcome
} catch {
    Write-Host "Run 'thocci on' to start mechanical keyboard sounds."
}
