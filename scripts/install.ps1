# Install the published Windows binary and start the current user's scheduled collector.
$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { throw 'Windows x86_64 is required.' }
if (-not (Get-Command git.exe -ErrorAction SilentlyContinue)) { throw 'Install Git for Windows first: https://git-scm.com/download/win' }
$version = if ($env:AIGC_VERSION) { $env:AIGC_VERSION } else { 'latest' }
$base = 'https://github.com/MyhreS/AI-Garbage-Collector/releases'
$base = if ($version -eq 'latest') { "$base/latest/download" } else { "$base/download/$version" }
$asset = 'aigc-x86_64-pc-windows-msvc.zip'
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('aigc-install-' + [Guid]::NewGuid())
$bin = Join-Path $env:LOCALAPPDATA 'aigc\bin'
try {
    New-Item -ItemType Directory -Path $scratch,$bin -Force | Out-Null
    Invoke-WebRequest "$base/$asset" -OutFile "$scratch\$asset" -UseBasicParsing
    Invoke-WebRequest "$base/SHA256SUMS" -OutFile "$scratch\SHA256SUMS" -UseBasicParsing
    $lines = @(Get-Content "$scratch\SHA256SUMS" | Where-Object { $_ -match ('^[a-fA-F0-9]{64}\s+' + [regex]::Escape($asset) + '$') })
    if ($lines.Count -ne 1) { throw 'Missing or ambiguous release checksum.' }
    $expected = ($lines[0] -split '\s+')[0]
    if ((Get-FileHash "$scratch\$asset" -Algorithm SHA256).Hash -ne $expected) { throw 'Release checksum mismatch.' }
    Expand-Archive "$scratch\$asset" -DestinationPath "$scratch\package"
    $task = Get-ScheduledTask -TaskName 'AI Garbage Collector' -ErrorAction SilentlyContinue
    if ($task) { Stop-ScheduledTask -InputObject $task }
    Copy-Item "$scratch\package\aigc.exe" "$bin\aigc.exe" -Force
    $userPath = [Environment]::GetEnvironmentVariable('Path','User')
    if (($userPath -split ';') -notcontains $bin) { [Environment]::SetEnvironmentVariable('Path', "$bin;$userPath", 'User') }
    if (($env:Path -split ';') -notcontains $bin) { $env:Path = "$bin;$env:Path" }
    & "$bin\aigc.exe" service install
    if ($LASTEXITCODE -ne 0) { throw 'Binary installed, but scheduler setup failed. Run aigc service install in your logged-in Windows session.' }
    Write-Host "Installed and scheduled hourly and at login. Status: & '$bin\aigc.exe' status"
    Write-Host 'Worktrees idle for seven days can be force-removed, including uncommitted files, without recovery.'
} finally {
    if (Test-Path $scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
}
