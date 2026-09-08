# sciink installer for Windows.
#   irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1 | iex
# Parameterized use (uninstall, pin a version, custom dest) needs a scriptblock,
# since a plain "| iex" one-liner cannot take parameters:
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1))) -Uninstall
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1))) -Version v0.1.0-alpha.1
# Or download and run:  .\install.ps1 [-Version v0.1.0] [-Dest <extensions dir>] [-Zip <local zip>] [-Uninstall]
param(
    [string]$Version = "latest",
    [string]$Zip = "",
    [string]$Dest = "",
    [switch]$Uninstall
)
$ErrorActionPreference = "Stop"
$ProgressPreference = 'SilentlyContinue'
try {
    # Windows PowerShell 5.1 defaults to TLS 1.0 on some hosts, which GitHub
    # rejects; PowerShell 7's default already includes TLS 1.2, so this is a
    # harmless no-op there.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {}
$repo = "Mr-Milk/sciink"
$asset = "sciink-windows-x64.zip"
if (-not $Dest) { $Dest = Join-Path $env:APPDATA "inkscape\extensions" }
$target = Join-Path $Dest "sciink"

if ($Uninstall) {
    if (Test-Path $target) { Remove-Item -Recurse -Force $target }
    Write-Host "sciink: removed $target"
    return
}

if ($Version -ne "latest") { $Version = "v" + ($Version -replace '^v', '') }

# Start of the install path: clear any staging leftovers from a crashed
# previous run before doing anything else. Inkscape scans subdirectories for
# .inx files, so a stray staging dir would create duplicate menu entries.
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
Get-ChildItem -Path $Dest -Filter ".sciink-stage-*" -Directory -Force -ErrorAction SilentlyContinue |
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# The download/version-check scratch dir can stay under the system temp dir;
# only the extraction stage below needs to be next to the final destination.
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("sciink-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null
$stage = Join-Path $Dest (".sciink-stage-" + [guid]::NewGuid().ToString("N"))
try {
    $zipPath = Join-Path $tmp $asset
    if ($Zip) {
        Copy-Item -Path $Zip -Destination $zipPath
    } else {
        if ($Version -eq "latest") {
            $url = "https://github.com/$repo/releases/latest/download/$asset"
            Write-Host "sciink: downloading $url"
            try {
                Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
            } catch {
                # GitHub's "latest" release excludes pre-releases, so this 404s
                # until the first stable release exists. Fall back to the
                # newest release of any kind.
                Write-Host "sciink: no stable release yet, checking for a pre-release"
                $releases = @(Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases?per_page=1" -UseBasicParsing)
                $tag = $releases[0].tag_name
                if (-not $tag) { throw "sciink: no releases found for $repo" }
                $url = "https://github.com/$repo/releases/download/$tag/$asset"
                Write-Host "sciink: downloading $url"
                Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
            }
        } else {
            $url = "https://github.com/$repo/releases/download/$Version/$asset"
            Write-Host "sciink: downloading $url"
            Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
        }
    }

    # Stage the extraction inside the destination directory (not the system
    # temp dir) so the final move below is a same-volume rename: PowerShell
    # 5.1's Move-Item refuses to move a directory across volumes ("Source and
    # destination path must have identical roots"). Validate the staged tree
    # fully before touching the real extensions directory, so a bad download
    # or archive never destroys an existing install.
    New-Item -ItemType Directory -Path $stage | Out-Null
    Expand-Archive -Path $zipPath -DestinationPath $stage -Force
    $stagedTarget = Join-Path $stage "sciink"
    $exe = Join-Path $stagedTarget "bin\sciink.exe"
    if (-not (Test-Path $exe)) { throw "sciink: archive did not contain sciink\bin\sciink.exe" }
    $versionFile = Join-Path $tmp "version.txt"
    $proc = Start-Process -FilePath $exe -ArgumentList "--version" -Wait -NoNewWindow -PassThru -RedirectStandardOutput $versionFile
    if ($proc.ExitCode -ne 0) { throw "sciink: the downloaded binary does not run on this system (exit code $($proc.ExitCode))" }
    $banner = ([string](Get-Content -Path $versionFile -Raw)).Trim()
    if (-not $banner.StartsWith("sciink ")) { throw "sciink: unexpected output from the downloaded binary: '$banner'" }

    if (Test-Path $target) { Remove-Item -Recurse -Force $target }
    Move-Item -Path $stagedTarget -Destination $Dest
    Write-Host "sciink: installed $banner into $target"
    Write-Host "sciink: restart Inkscape; the tools are under Extensions > Scientific"
} finally {
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
