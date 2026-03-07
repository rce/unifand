$ErrorActionPreference = "Continue"

$installDir = "C:\Program Files\Lian-Li\L-Connect 3"
$outDir = "C:\Users\rce\Desktop\decompiled"

# Install dotnet SDK first, then ilspycmd
$dotnetInstaller = "$env:TEMP\dotnet-install.ps1"
Write-Host "Downloading dotnet install script..."
Invoke-WebRequest -Uri "https://dot.net/v1/dotnet-install.ps1" -OutFile $dotnetInstaller

Write-Host "Installing .NET SDK..."
& $dotnetInstaller -Channel 8.0

# Add dotnet to PATH
$env:DOTNET_ROOT = "$env:LOCALAPPDATA\Microsoft\dotnet"
$env:PATH = "$env:DOTNET_ROOT;$env:DOTNET_ROOT\tools;$env:PATH"

Write-Host "Installing ilspycmd..."
& dotnet tool install --global ilspycmd

# Key DLLs to decompile
$targets = @(
    "L-Connect.Core.dll",
    "lianli.slv3.dll",
    "lianli.lcd207.dll",
    "lianli.ThemeEngine.dll",
    "Instances.dll",
    "LedIoControl.dll",
    "cled.dll",
    "monocled.dll",
    "GHidApi.dll",
    "L-Connect-Service.exe"
)

New-Item -ItemType Directory -Force -Path $outDir | Out-Null

foreach ($dll in $targets) {
    $src = Join-Path $installDir $dll
    if (Test-Path $src) {
        $name = [System.IO.Path]::GetFileNameWithoutExtension($dll)
        $dest = Join-Path $outDir $name
        Write-Host "Decompiling $dll..."
        & "$env:USERPROFILE\.dotnet\tools\ilspycmd.exe" $src -p -o $dest 2>&1
    } else {
        Write-Host "SKIP: $dll not found"
    }
}

Write-Host "Done! Output in $outDir"
