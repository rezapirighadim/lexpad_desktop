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
Add-Type -AssemblyName System.Drawing, System.Windows.Forms, UIAutomationClient, UIAutomationTypes
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
function Quit-Item {
  $clock = [System.Diagnostics.Stopwatch]::StartNew()
  $said = $false
  while ($clock.Elapsed.TotalSeconds -lt 15) {
    # Each top-level window on its own: the jump list is its own window
    # (XAML, in the shell's process), and one slow window must not hide it.
    foreach ($top in @($UIA::RootElement.FindAll($Tree::Children, $All))) {
      try {
        $item = $top.FindAll($Tree::Descendants, $All) |
          Where-Object { $_.Current.Name -like '*Quit Lexpad*' -and $_.Current.BoundingRectangle.Width -gt 0 } | Select-Object -First 1
      } catch { $item = $null }
      if ($item) {
        Write-Host "JUMP found '$($item.Current.Name)' in '$($top.Current.Name)' ($($top.Current.ClassName))"
        return $item
      }
    }
    foreach ($h in [JumpWin]::Visible()) {
      $cls = [JumpWin]::ClassOf($h); $title = [JumpWin]::TitleOf($h)
      if ($cls -notlike '*CoreWindow*' -and $title -notlike '*Jump*' -and $cls -notlike '*Xaml*' -and $cls -notlike '*Popup*') { continue }
      try {
        $item = $UIA::FromHandle($h).FindAll($Tree::Descendants, $All) |
          Where-Object { $_.Current.Name -like '*Quit Lexpad*' -and $_.Current.BoundingRectangle.Width -gt 0 } | Select-Object -First 1
      } catch { $item = $null }
      if ($item) {
        Write-Host "JUMP found '$($item.Current.Name)' in window '$title' ($cls)"
        return $item
      }
    }
    if (-not $said) {
      $vis = @([JumpWin]::Visible() | ForEach-Object { "'$([JumpWin]::TitleOf($_))' ($([JumpWin]::ClassOf($_)))" })
      Write-Host "JUMP visible windows: $($vis -join ', ')"
      $tops = @($UIA::RootElement.FindAll($Tree::Children, $All) | ForEach-Object { "'$($_.Current.Name)' ($($_.Current.ClassName))" })
      Write-Host "JUMP top-level windows: $($tops -join ', ')"
      $said = $true
    }
    Start-Sleep -Milliseconds 500
  }
  return $null
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
if ($buttons.Count -eq 0) {
  Shot '2-no-button'
  Fail 'Lexpad has no taskbar button'
} else {
  $xy = Center $buttons[0]
  Write-Host "JUMP right-click '$($buttons[0].Current.Name)' at $($xy[0]),$($xy[1])"
  Click $xy[0] $xy[1] 'right'
  Start-Sleep -Seconds 2
  $item = Quit-Item
  Shot '2-jump-list'
  if (-not $item) {
    Fail 'the taskbar menu has no Quit Lexpad'
    Escape
  } else {
    $q = Center $item
    Write-Host "JUMP Quit Lexpad at $($q[0]),$($q[1])"
    Click $q[0] $q[1] 'left'
    if ($p.WaitForExit(15000)) {
      Write-Host "JUMP Quit Lexpad: Lexpad quit (exit code $($p.ExitCode))"
    } else {
      Shot '3-after-quit'
      Fail 'Quit Lexpad from the taskbar did not quit within 15 seconds'
    }
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
