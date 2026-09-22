$ErrorActionPreference = 'Stop'

$wsl = Join-Path $env:WINDIR 'System32\wsl.exe'
$alreadyRunning = Get-CimInstance Win32_Process -Filter "Name = 'wsl.exe'" |
    Where-Object { $_.CommandLine -match '-d Ubuntu -u liang --exec /usr/bin/sleep infinity' }

if (-not $alreadyRunning) {
    Start-Process -FilePath $wsl `
        -ArgumentList '-d', 'Ubuntu', '-u', 'liang', '--exec', '/usr/bin/sleep', 'infinity' `
        -WindowStyle Hidden
}
