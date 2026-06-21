$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$MinecraftSrcDir = [System.IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $ScriptDir) "minecraft-src"))
$VersionManifestUrl = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"

try {
    $Manifest = Invoke-RestMethod -Uri $VersionManifestUrl
    $Releases = $Manifest.versions | Where-Object { $_.type -eq "release" }
    $LatestVer = $Releases[1].id
}
catch {
    Write-Error "Failed to fetch version manifest or parse release versions from $VersionManifestUrl`nError: $_"
    exit 1
}

if (-not $LatestVer) {
    Write-Error "Failed to fetch second latest Minecraft release from $VersionManifestUrl"
    exit 1
}

Write-Host "Using $LatestVer as minimum Minecraft release"

$TempDir = Join-Path $ScriptDir ".gitcraft-tmp"
if (Test-Path $TempDir) {
    Remove-Item -Recurse -Force $TempDir
}
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

$OriginalLocation = Get-Location

try {
    Write-Host "Cloning GitCraft into $TempDir..."
    git clone https://github.com/WinPlay02/GitCraft (Join-Path $TempDir "GitCraft")
    if ($LASTEXITCODE -ne 0) {
        throw "git clone failed with exit code $LASTEXITCODE"
    }

    $BuildGradlePath = Join-Path $TempDir "GitCraft/build.gradle"
    if (Test-Path $BuildGradlePath) {
        $GradleContent = [System.IO.File]::ReadAllText($BuildGradlePath)
        $GradleContent = $GradleContent.Replace("-Xmx4G", "-Xmx8G")
        $Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($BuildGradlePath, $GradleContent, $Utf8NoBom)
    }
    else {
        throw "build.gradle not found at $BuildGradlePath"
    }

    Set-Location (Join-Path $TempDir "GitCraft")
    Write-Host "Running GitCraft..."
    
    $GitCraftArgs = @(
        "--override-repo-target=$MinecraftSrcDir",
        "--only-unobfuscated",
        "--mappings=identity_unmapped",
        "--min-version=$LatestVer",
        "--only-stable"
    )
    $ArgsString = $GitCraftArgs -join " "

    $BatPath = Join-Path (Get-Location) "run_gitcraft.bat"
    $BatContent = "@echo off`r`ncall gradlew.bat run --args=`"$ArgsString`"`r`nexit /b %ERRORLEVEL%`r`n"
    [System.IO.File]::WriteAllText($BatPath, $BatContent, (New-Object System.Text.UTF8Encoding($false)))

    cmd.exe /c run_gitcraft.bat
    $ExitCode = $LASTEXITCODE

    if (Test-Path $BatPath) {
        Remove-Item -Force $BatPath
    }

    if ($ExitCode -ne 0) {
        throw "gradlew failed with exit code $ExitCode"
    }

    # Clean up on success
    if (Test-Path $TempDir) {
        Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
    }
}
finally {
    Set-Location $OriginalLocation
}

Write-Host "Done! minecraft-src has been updated."
