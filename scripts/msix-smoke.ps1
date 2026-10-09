# CI's smoke test of the Microsoft Store's MSIX, on the Windows runner only.
#
# The Store signs the package it publishes; to install it here, a copy is
# signed with a throwaway self-signed certificate whose subject is the
# manifest's Publisher (made, trusted and used only on this runner; the
# uploaded .msix stays unsigned). Then:
#
# 1. Add-AppxPackage, start Lexpad from its package, and require it to be
#    running 20 seconds later.
# 2. Start on login: the app turns its StartupTask on at start (the setting
#    is on by default), so a process inside the package must read the task
#    as Enabled.
# 3. Sign-in: a process inside the package listens on 127.0.0.1 as auth.rs
#    does, and a request from outside the package (the browser's redirect)
#    must reach it.
#
# Leaves the signed copy at $env:RUNNER_TEMP/Lexpad-test.msix (for WACK)
# and the package installed.
param([Parameter(Mandatory)] [string]$Msix)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$kits = 'C:\Program Files (x86)\Windows Kits\10\bin'
$bin = Get-ChildItem $kits -Directory | Where-Object { $_.Name -match '^10\.' -and (Test-Path (Join-Path $_.FullName 'x64\signtool.exe')) } |
  Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
$signtool = Join-Path $bin.FullName 'x64\signtool.exe'

[xml]$manifest = Get-Content (Join-Path $root 'msix/AppxManifest.xml')
$identity = $manifest.Package.Identity
$appId = $manifest.Package.Applications.Application.Id
$taskId = $manifest.Package.Applications.Application.Extensions.Extension.StartupTask.TaskId

# A test certificate for this runner only.
$cert = New-SelfSignedCertificate -Type Custom -Subject $identity.Publisher -KeyUsage DigitalSignature `
  -FriendlyName 'Lexpad CI test only' -CertStoreLocation Cert:\CurrentUser\My `
  -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}') -NotAfter (Get-Date).AddDays(1)
$cer = Join-Path $env:RUNNER_TEMP 'lexpad-test.cer'
Export-Certificate -Cert $cert -FilePath $cer | Out-Null
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null
$test = Join-Path $env:RUNNER_TEMP 'Lexpad-test.msix'
Copy-Item $Msix $test -Force
& $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $test
if ($LASTEXITCODE) { throw "signtool failed ($LASTEXITCODE)" }

# 1. Install and start.
Add-AppxPackage -Path $test
$pkg = Get-AppxPackage -Name $identity.Name
if (-not $pkg) { throw 'the package was not installed' }
Write-Host "Installed $($pkg.PackageFullName) at $($pkg.InstallLocation)"
Start-Process "shell:AppsFolder\$($pkg.PackageFamilyName)!$appId"
Start-Sleep -Seconds 20
$running = Get-Process -Name Lexpad -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.Path.StartsWith($pkg.InstallLocation, [StringComparison]::OrdinalIgnoreCase) }
if (-not $running) { Get-Process | Sort-Object Name | Format-Table Name, Id, Path -AutoSize | Out-String | Write-Host; throw 'Lexpad from the package is not running 20 seconds after start' }
Write-Host "Lexpad from the package is running (pid $($running[0].Id))"

# Runs a Windows PowerShell script inside the package (its identity, its
# virtualized view) and waits for it to write its answer to $out.
$work = 'C:\lexpad-msix-ci'
New-Item -ItemType Directory -Path $work -Force | Out-Null
function Invoke-InPackage([string]$script, [string]$out, [int]$seconds = 60) {
  Remove-Item $out -ErrorAction SilentlyContinue
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
  Invoke-CommandInDesktopPackage -PackageFamilyName $pkg.PackageFamilyName -AppId $appId `
    -Command "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Args "-NoProfile -EncodedCommand $encoded"
  $deadline = (Get-Date).AddSeconds($seconds)
  while (-not (Test-Path $out) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
}

# 2. The StartupTask, as the package sees it.
$stateFile = Join-Path $work 'startup.txt'
Invoke-InPackage @"
try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime
  `$null = [Windows.ApplicationModel.StartupTask, Windows.ApplicationModel, ContentType = WindowsRuntime]
  `$asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { `$_.Name -eq 'AsTask' -and `$_.GetParameters().Count -eq 1 -and `$_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation``1' } | Select-Object -First 1
  `$t = `$asTask.MakeGenericMethod([Windows.ApplicationModel.StartupTask]).Invoke(`$null, @([Windows.ApplicationModel.StartupTask]::GetAsync('$taskId')))
  `$t.Wait()
  Set-Content -Path '$stateFile' -Value `$t.Result.State
} catch { Set-Content -Path '$stateFile' -Value ("error: " + `$_) }
"@ $stateFile
$state = if (Test-Path $stateFile) { (Get-Content $stateFile -Raw).Trim() } else { 'no answer' }
Write-Host "StartupTask '$taskId': $state"
if ($state -ne 'Enabled') { throw "start on login is not on inside the package: $state" }

# 3. A loopback listener inside the package, reached from outside it.
$portFile = Join-Path $work 'port.txt'
$gotFile = Join-Path $work 'got.txt'
Remove-Item $gotFile -ErrorAction SilentlyContinue
Invoke-InPackage @"
`$l = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
`$l.Start()
Set-Content -Path '$portFile' -Value `$l.LocalEndpoint.Port
`$task = `$l.AcceptTcpClientAsync()
if (`$task.Wait(60000)) {
  `$c = `$task.Result; `$s = `$c.GetStream()
  `$buf = New-Object byte[] 4096; `$n = `$s.Read(`$buf, 0, `$buf.Length)
  `$line = ([Text.Encoding]::ASCII.GetString(`$buf, 0, `$n) -split "``r``n")[0]
  `$body = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 200 OK``r``nContent-Length: 2``r``nConnection: close``r``n``r``nok")
  `$s.Write(`$body, 0, `$body.Length); `$c.Close()
  Set-Content -Path '$gotFile' -Value `$line
}
`$l.Stop()
"@ $portFile
if (-not (Test-Path $portFile)) { throw 'the in-package listener did not start' }
$port = (Get-Content $portFile -Raw).Trim()
$answer = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:$port/callback?state=ci&code=ci" -TimeoutSec 30
$deadline = (Get-Date).AddSeconds(15)
while (-not (Test-Path $gotFile) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 300 }
$got = if (Test-Path $gotFile) { (Get-Content $gotFile -Raw).Trim() } else { '' }
Write-Host "Loopback inside the package on port ${port}: answered '$($answer.Content)', received '$got'"
if ($got -notlike 'GET /callback?state=ci&code=ci HTTP/1.1') { throw 'the redirect did not reach the listener inside the package' }

Get-Process -Name Lexpad -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
Write-Host 'MSIX smoke test passed'
