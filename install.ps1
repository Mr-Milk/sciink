# sciink installer for Windows.
#   irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1 | iex
# Or download and run:  .\install.ps1 [-Version v0.1.0] [-Dest <extensions dir>] [-Zip <local zip>] [-Uninstall]
param(
    [string]$Version = "latest",
    [string]$Zip = "",
    [string]$Dest = "",
    [switch]$Uninstall
)
$ErrorActionPreference = "Stop"
$repo = "Mr-Milk/sciink"
$asset = "sciink-windows-x64.zip"
if (-not $Dest) { $Dest = Join-Path $env:APPDATA "inkscape\extensions" }
$target = Join-Path $Dest "sciink"

if ($Uninstall) {
    if (Test-Path $target) { Remove-Item -Recurse -Force $target }
    Write-Host "sciink: removed $target"
    exit 0
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("sciink-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zipPath = Join-Path $tmp $asset
    if ($Zip) {
        Copy-Item -Path $Zip -Destination $zipPath
    } else {
        $url = if ($Version -eq "latest") { "https://github.com/$repo/releases/latest/download/$asset" } else { "https://github.com/$repo/releases/download/$Version/$asset" }
        Write-Host "sciink: downloading $url"
        Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
    }

    # Stage the extraction and validate it fully before touching the real
    # extensions directory, so a bad download or archive never destroys an
    # existing install.
    $stage = Join-Path $tmp "stage"
    New-Item -ItemType Directory -Path $stage | Out-Null
    Expand-Archive -Path $zipPath -DestinationPath $stage -Force
    $stagedTarget = Join-Path $stage "sciink"
    $exe = Join-Path $stagedTarget "bin\sciink.exe"
    if (-not (Test-Path $exe)) { throw "sciink: archive did not contain sciink\bin\sciink.exe" }
    $versionFile = Join-Path $tmp "version.txt"
    $proc = Start-Process -FilePath $exe -ArgumentList "--version" -Wait -NoNewWindow -PassThru -RedirectStandardOutput $versionFile
    if ($proc.ExitCode -ne 0) { throw "sciink: the downloaded binary does not run on this system (exit code $($proc.ExitCode))" }
    $banner = (Get-Content -Path $versionFile -Raw).Trim()
    if (-not $banner.StartsWith("sciink ")) { throw "sciink: unexpected output from the downloaded binary: '$banner'" }

    if (Test-Path $target) { Remove-Item -Recurse -Force $target }
    New-Item -ItemType Directory -Force -Path $Dest | Out-Null
    Move-Item -Path $stagedTarget -Destination $Dest
    Write-Host "sciink: installed $banner into $target"
    Write-Host "sciink: restart Inkscape; the tools are under Extensions > Scientific"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
