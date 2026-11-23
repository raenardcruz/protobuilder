<#
.SYNOPSIS
    ProtoBuilder installer script for Windows (PowerShell 5.1+ and PowerShell 7+).

.DESCRIPTION
    Downloads the latest (or specified) ProtoBuilder release archive for Windows,
    falls back to building from source with Cargo if no prebuilt archive is available,
    installs protobuilder.exe, and registers PROTOBUILDER_HOME and PATH in the
    current user's Windows environment variables.

.EXAMPLE
    irm https://raw.githubusercontent.com/raenardcruz/protobuilder/main/install.ps1 | iex

.EXAMPLE
    .\install.ps1 -Version v1.0.0 -InstallDir "$HOME\.local\bin"
#>
param(
    [string]$Version = $env:PROTOBUILDER_VERSION,
    [string]$HomeDir = $env:PROTOBUILDER_HOME,
    [string]$InstallDir = $env:PROTOBUILDER_INSTALL_DIR,
    [string]$Repo = $env:PROTOBUILDER_REPO,
    [switch]$FromSource,
    [switch]$NoModifyPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Ensure TLS 1.2 on Windows PowerShell 5.1
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {
    # Ignore on modern .NET runtimes where TLS is managed by the OS
}

if ([string]::IsNullOrWhiteSpace($Repo)) {
    $Repo = 'raenardcruz/protobuilder'
}
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = 'latest'
}
if ([string]::IsNullOrWhiteSpace($HomeDir)) {
    $HomeDir = Join-Path $HOME '.protobuilder'
}
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path $HomeDir 'bin'
}
$UseFromSource = [bool]$FromSource -or ($env:PROTOBUILDER_FROM_SOURCE -eq '1')
$SkipModifyPath = [bool]$NoModifyPath -or ($env:PROTOBUILDER_NO_MODIFY_PATH -eq '1')

$BinName = 'protobuilder.exe'
$HomeDir = $HomeDir.TrimEnd('\', '/')
$InstallDir = $InstallDir.TrimEnd('\', '/')

function Write-Info {
    param([string]$Message)
    Write-Host "==> " -ForegroundColor Cyan -NoNewline
    Write-Host $Message
}

function Write-Success {
    param([string]$Message)
    Write-Host "==> " -ForegroundColor Green -NoNewline
    Write-Host $Message
}

function Write-Warn {
    param([string]$Message)
    Write-Host "warning: " -ForegroundColor Yellow -NoNewline
    Write-Host $Message
}

function Get-HostArch {
    $arch = $null
    try {
        $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    } catch {
        $arch = $env:PROCESSOR_ARCHITECTURE
    }

    switch -Regex ($arch) {
        '^(Arm64|ARM64)$' { return 'arm64' }
        '^(X64|AMD64|x86_64)$' { return 'x86_64' }
        default { throw "Unsupported Windows architecture: $arch" }
    }
}

function Resolve-ReleaseVersion {
    param(
        [string]$RequestedVersion,
        [string]$Repository
    )

    if ($RequestedVersion -ne 'latest') {
        if ($RequestedVersion -match '^v') {
            return $RequestedVersion
        }
        return "v$RequestedVersion"
    }

    Write-Info "Resolving latest release version from github.com/$Repository..."

    # Try redirect first to avoid GitHub REST API rate limits
    try {
        $req = [System.Net.HttpWebRequest]::Create("https://github.com/$Repository/releases/latest")
        $req.AllowAutoRedirect = $false
        $req.UserAgent = 'protobuilder-installer'
        $resp = $req.GetResponse()
        $location = if ($resp.Headers) { $resp.Headers['Location'] } else { $null }
        $resp.Close()
        if ($location -and ($location -match '/releases/tag/([^/?#]+)')) {
            return $Matches[1]
        }
    } catch [System.Net.WebException] {
        if ($_.Exception.Response) {
            $location = if ($_.Exception.Response.Headers) { $_.Exception.Response.Headers['Location'] } else { $null }
            $_.Exception.Response.Close()
            if ($location -and ($location -match '/releases/tag/([^/?#]+)')) {
                return $Matches[1]
            }
        }
    }

    # Fallback to GitHub REST API
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/latest" -Headers @{ 'User-Agent' = 'protobuilder-installer' }
        if ($release.tag_name) {
            return $release.tag_name
        }
    } catch {
        # Handled below
    }

    throw "Could not determine the latest release version. Specify one explicitly with -Version (e.g. -Version v1.0.0)."
}

function Download-File {
    param(
        [string]$Url,
        [string]$OutFile
    )

    $prevProgress = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    try {
        Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing -Headers @{ 'User-Agent' = 'protobuilder-installer' }
        return $true
    } catch {
        return $false
    } finally {
        $ProgressPreference = $prevProgress
    }
}

function Try-DownloadPrebuilt {
    param(
        [string]$Repository,
        [string]$ReleaseVersion,
        [string]$Arch,
        [string]$TempRoot
    )

    $candidates = @()
    if ($Arch -eq 'arm64') {
        $candidates += "protobuilder-$ReleaseVersion-windows-arm64.zip"
        $candidates += "protobuilder-$ReleaseVersion-windows-aarch64.zip"
        # Windows 11 ARM64 natively runs x86_64 binaries via emulation
        $candidates += "protobuilder-$ReleaseVersion-windows-x86_64.zip"
    } else {
        $candidates += "protobuilder-$ReleaseVersion-windows-x86_64.zip"
        $candidates += "protobuilder-$ReleaseVersion-windows-amd64.zip"
    }

    $archivePath = Join-Path $TempRoot 'protobuilder.zip'
    $extractDir = Join-Path $TempRoot 'extract'
    New-Item -ItemType Directory -Force -Path $extractDir | Out-Null

    $downloaded = $false
    foreach ($asset in $candidates) {
        $url = "https://github.com/$Repository/releases/download/$ReleaseVersion/$asset"
        Write-Info "Checking release archive: $asset"
        if (Download-File -Url $url -OutFile $archivePath) {
            $downloaded = $true
            break
        }
    }

    if (-not $downloaded) {
        return $null
    }

    Write-Info "Extracting $BinName..."
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force

    $binFile = Get-ChildItem -Path $extractDir -Filter $BinName -File -Recurse | Select-Object -First 1
    if (-not $binFile) {
        throw "Release archive was downloaded, but '$BinName' was not found inside."
    }

    return $binFile.FullName
}

function Build-FromSource {
    param(
        [string]$Repository,
        [string]$ReleaseVersion,
        [string]$Arch,
        [string]$TempRoot
    )

    $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargoCmd) {
        throw @"
No prebuilt Windows binary is available for windows-$Arch in $ReleaseVersion, and 'cargo' is not installed.
Install Rust via https://rustup.rs and re-run this script, or download a release manually from:
  https://github.com/$Repository/releases
"@
    }

    $srcArchive = Join-Path $TempRoot 'source.zip'
    $srcExtract = Join-Path $TempRoot 'src'
    New-Item -ItemType Directory -Force -Path $srcExtract | Out-Null

    $srcUrl = "https://github.com/$Repository/archive/refs/tags/$ReleaseVersion.zip"
    Write-Info "Downloading source archive for $ReleaseVersion..."
    if (-not (Download-File -Url $srcUrl -OutFile $srcArchive)) {
        throw "Failed to download source archive from $srcUrl"
    }

    Expand-Archive -Path $srcArchive -DestinationPath $srcExtract -Force
    $cargoToml = Get-ChildItem -Path $srcExtract -Filter 'Cargo.toml' -File -Recurse -Depth 3 | Select-Object -First 1
    if (-not $cargoToml) {
        throw "Could not locate Cargo.toml in extracted source archive."
    }

    $srcDir = $cargoToml.Directory.FullName
    Write-Info "Building $BinName from source with Cargo (release mode)..."
    Push-Location $srcDir
    try {
        & cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo build failed with exit code $LASTEXITCODE."
        }
    } finally {
        Pop-Location
    }

    $builtBin = Join-Path $srcDir "target\release\$BinName"
    if (-not (Test-Path $builtBin)) {
        throw "Cargo build completed, but binary was not found at $builtBin."
    }

    return $builtBin
}

function Register-UserEnv {
    param(
        [string]$PBHome,
        [string]$BinDir,
        [bool]$SkipModify
    )

    if ($SkipModify) {
        Write-Info "Skipping environment variable registration (-NoModifyPath)."
        return
    }

    # Register PROTOBUILDER_HOME
    $existingHome = [Environment]::GetEnvironmentVariable('PROTOBUILDER_HOME', 'User')
    if ($existingHome -ne $PBHome) {
        [Environment]::SetEnvironmentVariable('PROTOBUILDER_HOME', $PBHome, 'User')
        Write-Success "Registered user environment variable PROTOBUILDER_HOME=$PBHome"
    } else {
        Write-Info "PROTOBUILDER_HOME is already registered ($PBHome)"
    }
    $env:PROTOBUILDER_HOME = $PBHome

    # Register InstallDir in User PATH (idempotent, case-insensitive)
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @()
    if (-not [string]::IsNullOrWhiteSpace($userPath)) {
        $entries = $userPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    }

    $normalizedTarget = $BinDir.TrimEnd('\', '/').ToLowerInvariant()
    $alreadyInUserPath = $false
    foreach ($entry in $entries) {
        if ($entry.TrimEnd('\', '/').ToLowerInvariant() -eq $normalizedTarget) {
            $alreadyInUserPath = $true
            break
        }
    }

    if (-not $alreadyInUserPath) {
        $newUserPath = (@($entries) + @($BinDir)) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
        Write-Success "Added $BinDir to User PATH"
    } else {
        Write-Info "$BinDir is already registered in User PATH"
    }

    # Also update current PowerShell session PATH so protobuilder works immediately
    $sessionEntries = @()
    if (-not [string]::IsNullOrWhiteSpace($env:Path)) {
        $sessionEntries = $env:Path -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    }
    $alreadyInSession = $false
    foreach ($entry in $sessionEntries) {
        if ($entry.TrimEnd('\', '/').ToLowerInvariant() -eq $normalizedTarget) {
            $alreadyInSession = $true
            break
        }
    }
    if (-not $alreadyInSession) {
        $env:Path = "$BinDir;$env:Path"
    }
}

# --- Main -------------------------------------------------------------------

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("protobuilder-install-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

try {
    $arch = Get-HostArch
    $resolvedVersion = Resolve-ReleaseVersion -RequestedVersion $Version -Repository $Repo

    Write-Info "Installing $BinName $resolvedVersion (windows-$arch)..."

    $builtBin = $null
    if ($UseFromSource) {
        $builtBin = Build-FromSource -Repository $Repo -ReleaseVersion $resolvedVersion -Arch $arch -TempRoot $tmpDir
    } else {
        $builtBin = Try-DownloadPrebuilt -Repository $Repo -ReleaseVersion $resolvedVersion -Arch $arch -TempRoot $tmpDir
        if (-not $builtBin) {
            Write-Warn "No prebuilt archive found for windows-$arch in $resolvedVersion. Falling back to building from source..."
            $builtBin = Build-FromSource -Repository $Repo -ReleaseVersion $resolvedVersion -Arch $arch -TempRoot $tmpDir
        }
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $destPath = Join-Path $InstallDir $BinName
    Copy-Item -Path $builtBin -Destination $destPath -Force
    Unblock-File -Path $destPath -ErrorAction SilentlyContinue
    Write-Success "Installed $BinName to $destPath"

    Register-UserEnv -PBHome $HomeDir -BinDir $InstallDir -SkipModify $SkipModifyPath

    try {
        $verOutput = & $destPath --version
        Write-Success "Verified: $verOutput"
    } catch {
        Write-Warn "Binary installed at $destPath, but '--version' check failed."
    }

    Write-Host ""
    Write-Host "ProtoBuilder is ready to use! Run:"
    Write-Host "  protobuilder --help"
    Write-Host ""
    Write-Host "Note: New terminals will automatically pick up PROTOBUILDER_HOME and PATH."
} finally {
    Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}
