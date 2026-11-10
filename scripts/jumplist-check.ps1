# CI (Windows): "Quit Lexpad" in the taskbar button's right-click menu (the
# Jump List task, src-tauri/src/jump_list.rs), with the real mouse:
#
#   1. Start Lexpad; its list is written (the shell's CustomDestinations file
#      holds "Quit Lexpad").
#   2. Right-click Lexpad's taskbar button: the menu shows "Quit Lexpad".
#   3. Click it: Lexpad is gone within 15 seconds.
#   4. `Lexpad.exe --quit` with no Lexpad running opens nothing and ends.
#
# Windows 10's taskbar (MSTaskListWClass) and Windows 11's (XAML) are both
# looked through by UI Automation. Screenshots go to jumplist-check/.
# Prints JUMP lines and "JUMP failures <n>"; exits 1 when any step failed.
param([Parameter(Mandatory = $true)] [string] $Exe)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing, System.Windows.Forms, UIAutomationClient, UIAutomationTypes, WindowsBase
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class JumpWin {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr after, string cls, string title);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
  public static string ClassOf(IntPtr h) { var s = new System.Text.StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static string TitleOf(IntPtr h) { var s = new System.Text.StringBuilder(256); GetWindowText(h, s, 256); return s.ToString(); }
  public static System.Collections.Generic.List<IntPtr> Visible() {
    var found = new System.Collections.Generic.List<IntPtr>();
    EnumWindows((h, l) => { if (IsWindowVisible(h)) found.Add(h); return true; }, IntPtr.Zero);
    return found;
  }
  public static System.Collections.Generic.List<IntPtr> Children(IntPtr p) {
    var found = new System.Collections.Generic.List<IntPtr>();
    EnumChildWindows(p, (h, l) => { found.Add(h); return true; }, IntPtr.Zero);
    return found;
  }
}
'@
[void][JumpWin]::SetProcessDPIAware()
$UIA = [System.Windows.Automation.AutomationElement]
$Tree = [System.Windows.Automation.TreeScope]
$All = [System.Windows.Automation.Condition]::TrueCondition

$out = Join-Path (Get-Location) 'jumplist-check'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$script:failures = 0
function Fail([string] $why) { Write-Host "JUMP FAIL $why"; $script:failures++ }
function Shot([string] $name) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}
function Click([int] $x, [int] $y, [string] $button) {
  [void][JumpWin]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 150
  if ($button -eq 'right') { $down = 0x0008; $up = 0x0010 } else { $down = 0x0002; $up = 0x0004 }
  [JumpWin]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 60
  [JumpWin]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
}
function Escape { [JumpWin]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [JumpWin]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero) }
function Center($el) { $b = $el.Current.BoundingRectangle; return @([int]($b.X + $b.Width / 2), [int]($b.Y + $b.Height / 2)) }

# Lexpad's taskbar buttons (the Player's and Lexpad's own window), never the
# notification area's icon, which is also called Lexpad.
function Taskbar-Buttons {
  $tw = [JumpWin]::FindWindow('Shell_TrayWnd', $null)
  $list = [IntPtr]::Zero
  if ($tw -ne [IntPtr]::Zero) {
    $rebar = [JumpWin]::FindWindowEx($tw, [IntPtr]::Zero, 'ReBarWindow32', $null)
    $sw = if ($rebar -ne [IntPtr]::Zero) { [JumpWin]::FindWindowEx($rebar, [IntPtr]::Zero, 'MSTaskSwWClass', $null) } else { [IntPtr]::Zero }
    $list = if ($sw -ne [IntPtr]::Zero) { [JumpWin]::FindWindowEx($sw, [IntPtr]::Zero, 'MSTaskListWClass', $null) } else { [IntPtr]::Zero }
  }
  if ($list -eq [IntPtr]::Zero -and $tw -ne [IntPtr]::Zero) {
    $list = [JumpWin]::Children($tw) | Where-Object { [JumpWin]::ClassOf($_) -eq 'MSTaskListWClass' } | Select-Object -First 1
    if (-not $list) { $list = [IntPtr]::Zero }
  }
  $kids = @(if ($tw -ne [IntPtr]::Zero) { [JumpWin]::Children($tw) | ForEach-Object { [JumpWin]::ClassOf($_) } })
  Write-Host "JUMP taskbar windows: $(($kids | Select-Object -Unique) -join ', '); task list $(if ($list -ne [IntPtr]::Zero) { 'found' } else { 'not found' })"
  $found = @()
  if ($list -ne [IntPtr]::Zero) {
    $found = @($UIA::FromHandle($list).FindAll($Tree::Descendants, $All) | Where-Object { $_.Current.Name })
  }
  if ($found.Count -eq 0 -and $tw -ne [IntPtr]::Zero) {
    # Windows 11's task list is XAML (its buttons' AutomationId start with
    # "Appid:"); on Server 2022 the task list's own element may list nothing.
    # Then every named button of the taskbar outside the notification area.
    $tn = [JumpWin]::FindWindowEx($tw, [IntPtr]::Zero, 'TrayNotifyWnd', $null)
    $notify = if ($tn -ne [IntPtr]::Zero) { $UIA::FromHandle($tn).Current.BoundingRectangle } else { $null }
    $found = @($UIA::FromHandle($tw).FindAll($Tree::Descendants, $All) | Where-Object {
      $n = $_.Current.Name; $b = $_.Current.BoundingRectangle
      $n -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and $b.Width -gt 0 -and
        -not ($notify -and $b.X -ge $notify.X -and $b.X -lt ($notify.X + $notify.Width))
    })
  }
  Write-Host "JUMP taskbar buttons: $((@($found | ForEach-Object { "'$($_.Current.Name)'" })) -join ', ')"
  return @($found | Where-Object { $_.Current.Name -like '*Lexpad*' })
}

# The open jump list's "Quit Lexpad", from the desktop down (it is its own
# window: XAML on Windows 10 1607+ and 11).
# The open jump list's "Quit Lexpad". The menu is the shell's own XAML
# window, which neither EnumWindows nor the desktop's UI Automation children
# list on Windows 11 (CI, 10 Nov 2026), so it is read where it is drawn:
# UI Automation at points straight above the button, as the pointer would go.
function Quit-Item([int] $x, [int] $top) {
  $clock = [System.Diagnostics.Stopwatch]::StartNew()
  $seen = @{}
  while ($clock.Elapsed.TotalSeconds -lt 15) {
    foreach ($dx in 0, -60, 60) {
      for ($y = $top - 4; $y -gt [Math]::Max(0, $top - 700); $y -= 6) {
        try { $el = $UIA::FromPoint((New-Object System.Windows.Point(($x + $dx), $y))) } catch { continue }
        if (-not $el) { continue }
        $name = $el.Current.Name
        if ($name) { $seen[$name] = 1 }
        if ($name -like '*Quit Lexpad*' -and $el.Current.BoundingRectangle.Width -gt 0) {
          Write-Host "JUMP found '$name' ($($el.Current.ClassName)) at $($x + $dx),$y"
          return $el
        }
      }
    }
    Start-Sleep -Milliseconds 300
  }
  Write-Host "JUMP names above the button: $((@($seen.Keys | ForEach-Object { "'$_'" })) -join ', ')"
  return $null
}

# Lexpad's taskbar button found by its icon's green, for a taskbar whose
# buttons UI Automation does not list (Server 2022 on CI): the densest run of
# that colour on the taskbar, left of the notification area.
function Button-By-Colour {
  $tw = [JumpWin]::FindWindow('Shell_TrayWnd', $null)
  $tb = $UIA::FromHandle($tw).Current.BoundingRectangle
  $tn = [JumpWin]::FindWindowEx($tw, [IntPtr]::Zero, 'TrayNotifyWnd', $null)
  $right = if ($tn -ne [IntPtr]::Zero) { [int]$UIA::FromHandle($tn).Current.BoundingRectangle.X } else { [int]($tb.X + $tb.Width) }
  $w = $right - [int]$tb.X; $h = [int]$tb.Height
  if ($w -le 0 -or $h -le 0) { return $null }
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen([int]$tb.X, [int]$tb.Y, 0, 0, $bmp.Size)
  $cols = New-Object int[] $w
  for ($x = 0; $x -lt $w; $x++) {
    for ($y = 0; $y -lt $h; $y += 2) {
      $c = $bmp.GetPixel($x, $y)
      if ([Math]::Abs($c.R - 46) + [Math]::Abs($c.G - 123) + [Math]::Abs($c.B - 78) -lt 45) { $cols[$x]++ }
    }
  }
  $g.Dispose(); $bmp.Dispose()
  $best = 0; $at = -1
  for ($x = 0; $x -lt $w - 24; $x++) {
    $sum = 0; for ($i = 0; $i -lt 24; $i++) { $sum += $cols[$x + $i] }
    if ($sum -gt $best) { $best = $sum; $at = $x }
  }
  Write-Host "JUMP icon colour on the taskbar: $best pixels at x $([int]$tb.X + $at)"
  if ($best -lt 30) { return $null }
  return @(([int]$tb.X + $at + 12), ([int]($tb.Y + $tb.Height / 2)), [int]$tb.Y)
}

# --- 1: start, and the list is written --------------------------------------
Get-Process lexpad-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
$since = Get-Date
$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 10
$dests = Join-Path $env:APPDATA 'Microsoft\Windows\Recent\CustomDestinations'
$lists = @(Get-ChildItem $dests -Filter *.customDestinations-ms -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $since.AddSeconds(-2) })
$withQuit = @($lists | Where-Object { [System.Text.Encoding]::Unicode.GetString([System.IO.File]::ReadAllBytes($_.FullName)).Contains('Quit Lexpad') })
Write-Host "JUMP lists written since the start: $($lists.Count), with Quit Lexpad: $($withQuit.Count)"
if ($withQuit.Count -lt 1) { Fail 'no jump list with Quit Lexpad was written' }

# --- 2 and 3: right-click the taskbar button, Quit Lexpad -------------------
$buttons = Taskbar-Buttons
$target = $null
if ($buttons.Count -gt 0) {
  $xy = Center $buttons[0]
  $target = @($xy[0], $xy[1], [int]$buttons[0].Current.BoundingRectangle.Y)
  Write-Host "JUMP right-click '$($buttons[0].Current.Name)' at $($xy[0]),$($xy[1])"
} else {
  $target = Button-By-Colour
  if ($target) { Write-Host "JUMP right-click the button with Lexpad's icon at $($target[0]),$($target[1])" }
}
if (-not $target) {
  Shot '2-no-button'
  Fail 'Lexpad has no taskbar button'
} else {
  Click $target[0] $target[1] 'right'
  Start-Sleep -Seconds 2
  Shot '2-jump-list'
  $item = Quit-Item $target[0] $target[2]
  if ($item) {
    $q = Center $item
    Write-Host "JUMP Quit Lexpad at $($q[0]),$($q[1])"
    Click $q[0] $q[1] 'left'
  } else {
    # The shell's menu is not readable here: its first row is Tasks' Quit
    # Lexpad (2-jump-list.png), so the keyboard picks it: Home, then Enter.
    # Only that row quits Lexpad: the others open the Player, pin or close a
    # window, and Lexpad stays in the tray after a window closes. Home, since
    # Windows 11 already has the first row focused and Windows 10 none.
    Write-Host 'JUMP choosing the first row by keyboard'
    [JumpWin]::keybd_event(0x24, 0, 0, [UIntPtr]::Zero); [JumpWin]::keybd_event(0x24, 0, 2, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    Shot '2-first-row'
    [JumpWin]::keybd_event(0x0D, 0, 0, [UIntPtr]::Zero); [JumpWin]::keybd_event(0x0D, 0, 2, [UIntPtr]::Zero)
  }
  if ($p.WaitForExit(15000)) {
    Write-Host "JUMP Quit Lexpad: Lexpad quit (exit code $($p.ExitCode))"
  } else {
    Shot '3-after-quit'
    Fail 'Quit Lexpad from the taskbar did not quit within 15 seconds'
    Escape
  }
}
Get-Process lexpad-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

# --- 4: --quit with nothing running -----------------------------------------
$q = Start-Process -FilePath $Exe -ArgumentList '--quit' -PassThru
if ($q.WaitForExit(15000)) {
  Write-Host "JUMP --quit alone ended (exit code $($q.ExitCode))"
  if (Get-Process lexpad-desktop -ErrorAction SilentlyContinue) { Fail '--quit alone left a Lexpad running' }
} else {
  Shot '4-quit-alone'
  Fail '--quit alone did not end within 15 seconds'
  Stop-Process -Id $q.Id -Force -ErrorAction SilentlyContinue
}

Write-Host "JUMP failures $script:failures"
if ($script:failures -gt 0) { exit 1 }
