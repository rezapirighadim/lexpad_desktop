# CI (Windows): the tray icon answers the mouse as a learner expects
# (docs/windows-manual-test.md, steps 17 and 20), with the real mouse on the
# real notification area, not with messages posted to the app.
#
#   1. Right-click the icon: a menu opens with "Open Lexpad" and "Quit Lexpad".
#   2. Click "Quit Lexpad": Lexpad is gone within 15 seconds.
#   3. Start it again and left-click the icon: the panel opens.
#
# The icon's place comes from Shell_NotifyIconGetRect for the window
# tray-icon makes (class tray_icon_app). A new icon sits in the overflow
# (under the ^ arrow) on the runner, as on a learner's Windows 11, where the
# shell gives the ^ button's place: then the check opens the overflow and
# clicks the icon there, as a learner would.
# Each step leaves a screenshot in tray-check/ (uploaded by the workflow).
#
# Prints TRAY lines and "TRAY failures <n>"; exits 1 when any step failed.
param([Parameter(Mandatory = $true)] [string] $Exe)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class TrayWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct NOTIFYICONIDENTIFIER { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetMenuString(IntPtr m, uint item, StringBuilder s, int max, uint flag);
  [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr h, IntPtr m, uint item, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER id, out RECT r);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr after, string cls, string title);

  public static string ClassOf(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static string TitleOf(IntPtr h) { var s = new StringBuilder(256); GetWindowText(h, s, 256); return s.ToString(); }
  public static uint PidOf(IntPtr h) { uint p; GetWindowThreadProcessId(h, out p); return p; }
  public static List<IntPtr> Windows(uint pid, string cls) {
    var found = new List<IntPtr>();
    EnumWindows((h, l) => { if (PidOf(h) == pid && (cls == null || ClassOf(h) == cls)) found.Add(h); return true; }, IntPtr.Zero);
    return found;
  }
  public static List<IntPtr> VisibleMenus() {
    var found = new List<IntPtr>();
    EnumWindows((h, l) => { if (ClassOf(h) == "#32768" && IsWindowVisible(h)) found.Add(h); return true; }, IntPtr.Zero);
    return found;
  }
  public static string[] Items(IntPtr menu) {
    int n = GetMenuItemCount(menu);
    var items = new string[Math.Max(n, 0)];
    for (uint i = 0; i < n; i++) { var s = new StringBuilder(256); GetMenuString(menu, i, s, 256, 0x400); items[i] = s.ToString(); }
    return items;
  }
}
'@
[void][TrayWin]::SetProcessDPIAware()

$out = Join-Path (Get-Location) 'tray-check'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$script:failures = 0
function Fail([string] $why) { Write-Host "TRAY FAIL $why"; $script:failures++ }
function Shot([string] $name) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}
function Click([int] $x, [int] $y, [string] $button) {
  [void][TrayWin]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 150
  if ($button -eq 'right') { $down = 0x0008; $up = 0x0010 } else { $down = 0x0002; $up = 0x0004 }
  [TrayWin]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 60
  [TrayWin]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
}
function Escape { [TrayWin]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [TrayWin]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero) }

function Start-Lexpad {
  Get-Process lexpad-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Seconds 1
  $p = Start-Process -FilePath $Exe -PassThru
  # The tray's window, once tray-icon has made it.
  for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Milliseconds 500
    $trays = @([TrayWin]::Windows([uint32]$p.Id, 'tray_icon_app'))
    if ($trays.Count -gt 0) { break }
  }
  if ($trays.Count -eq 0) { throw "Lexpad (pid $($p.Id)) made no tray window" }
  $tray = $trays[0]
  Write-Host "TRAY pid $($p.Id): $($trays.Count) tray window(s) after $([int]($i / 2)) s"
  Start-Sleep -Seconds 4
  # Whatever window the launch opened is put away, so the clicks go to the tray.
  foreach ($h in [TrayWin]::Windows([uint32]$p.Id, $null)) {
    if ([TrayWin]::IsWindowVisible($h) -and [TrayWin]::ClassOf($h) -ne 'tray_icon_app') {
      Write-Host "TRAY launch window '$([TrayWin]::TitleOf($h))' ($([TrayWin]::ClassOf($h)))"
    }
  }
  return @{ Process = $p; Tray = $tray }
}

# The icon's rectangle in screen pixels, or $null when the shell has no
# place for it (hidden in the overflow).
function Icon-Rect($tray) {
  for ($id = 0; $id -lt 8; $id++) {
    $nii = New-Object TrayWin+NOTIFYICONIDENTIFIER
    $nii.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($nii)
    $nii.hWnd = $tray
    $nii.uID = $id
    $r = New-Object TrayWin+RECT
    $hr = [TrayWin]::Shell_NotifyIconGetRect([ref]$nii, [ref]$r)
    if ($hr -eq 0 -and $r.Right -gt $r.Left) {
      Write-Host "TRAY icon uID $id at $($r.Left),$($r.Top) $($r.Right - $r.Left)x$($r.Bottom - $r.Top)"
      return $r
    }
  }
  return $null
}

# The ^ button that opens the overflow ("Show hidden icons"), by UI Automation.
function Chevron {
  Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $all = [System.Windows.Automation.Condition]::TrueCondition
  return $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $all) |
    Where-Object { $_.Current.Name -match '^(Show hidden icons|Hide hidden icons|Notification Chevron)' } | Select-Object -First 1
}

function Inside($r, $b) {
  $cx = ($r.Left + $r.Right) / 2; $cy = ($r.Top + $r.Bottom) / 2
  return $cx -ge $b.X -and $cx -le ($b.X + $b.Width) -and $cy -ge $b.Y -and $cy -le ($b.Y + $b.Height)
}

# The notification area's own windows (the Windows 10 taskbar; Windows 11
# draws its tray in XAML and has none of these, so each may be zero).
function Taskbar-Parts {
  $tw = [TrayWin]::FindWindow('Shell_TrayWnd', $null)
  $tn = if ($tw -ne [IntPtr]::Zero) { [TrayWin]::FindWindowEx($tw, [IntPtr]::Zero, 'TrayNotifyWnd', $null) } else { [IntPtr]::Zero }
  $chev = if ($tn -ne [IntPtr]::Zero) { [TrayWin]::FindWindowEx($tn, [IntPtr]::Zero, 'Button', $null) } else { [IntPtr]::Zero }
  $pager = if ($tn -ne [IntPtr]::Zero) { [TrayWin]::FindWindowEx($tn, [IntPtr]::Zero, 'SysPager', $null) } else { [IntPtr]::Zero }
  $shown = if ($pager -ne [IntPtr]::Zero) { [TrayWin]::FindWindowEx($pager, [IntPtr]::Zero, 'ToolbarWindow32', $null) } else { [IntPtr]::Zero }
  $ow = [TrayWin]::FindWindow('NotifyIconOverflowWindow', $null)
  $hidden = if ($ow -ne [IntPtr]::Zero) { [TrayWin]::FindWindowEx($ow, [IntPtr]::Zero, 'ToolbarWindow32', $null) } else { [IntPtr]::Zero }
  return @{ Chevron = $chev; Shown = $shown; Overflow = $ow; Hidden = $hidden }
}

# The icons in one of those toolbars, by UI Automation: name and place.
function Toolbar-Icons([IntPtr] $toolbar) {
  if ($toolbar -eq [IntPtr]::Zero) { return @() }
  Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
  $el = [System.Windows.Automation.AutomationElement]::FromHandle($toolbar)
  $all = [System.Windows.Automation.Condition]::TrueCondition
  return @($el.FindAll([System.Windows.Automation.TreeScope]::Children, $all) | ForEach-Object {
    $b = $_.Current.BoundingRectangle
    [pscustomobject]@{ Name = $_.Current.Name; X = [int]$b.X; Y = [int]$b.Y; W = [int]$b.Width; H = [int]$b.Height }
  })
}

function Say-Icons([string] $where, $icons) {
  $names = @($icons | ForEach-Object { "'$($_.Name)'" })
  Write-Host "TRAY icons $where ($($names.Count)): $($names -join ', ')"
}

function Rect-Of($b) {
  $r = New-Object TrayWin+RECT
  $r.Left = [int]$b.X; $r.Top = [int]$b.Y; $r.Right = [int]($b.X + $b.W); $r.Bottom = [int]($b.Y + $b.H)
  return $r
}

# The icon's own rectangle. A new icon may sit in the overflow (under the ^
# arrow), where the shell answers with no rectangle or the ^ button's: then
# the overflow is opened, as a learner would, and the icon is looked for in
# it by name. When it is in neither the taskbar's icons nor the overflow, the
# check says so: the icon is really missing, not hidden.
function Find-Icon($tray, [string] $tag) {
  # A fresh start may take a few seconds to put its icon in the shell. Up to
  # 60 s, and how long it took is printed: a restart right after Quit on the
  # Windows 10 taskbar found none within 10 s (0.3.30), and this tells a slow
  # shell from an icon that is never added.
  $r = $null
  $clock = [System.Diagnostics.Stopwatch]::StartNew()
  while (-not $r -and $clock.Elapsed.TotalSeconds -lt 60) {
    $r = Icon-Rect $tray
    if (-not $r) { Start-Sleep -Milliseconds 500 }
  }
  Write-Host "TRAY icon rect after $([int]$clock.Elapsed.TotalSeconds) s: $(if ($r) { 'found' } else { 'none' })"
  $parts = Taskbar-Parts
  $shown = Toolbar-Icons $parts.Shown
  if ($parts.Shown -ne [IntPtr]::Zero) { Say-Icons 'on the taskbar' $shown }
  $chevron = Chevron
  $cb = $null
  if ($chevron) {
    $b = $chevron.Current.BoundingRectangle
    $cb = [pscustomobject]@{ X = $b.X; Y = $b.Y; Width = $b.Width; Height = $b.Height; Name = $chevron.Current.Name }
  } elseif ($parts.Chevron -ne [IntPtr]::Zero -and [TrayWin]::IsWindowVisible($parts.Chevron)) {
    # The Windows 10 taskbar's ^ is a plain Button in TrayNotifyWnd; UI
    # Automation names it differently by language and build.
    $wr = New-Object TrayWin+RECT
    [void][TrayWin]::GetWindowRect($parts.Chevron, [ref]$wr)
    $cb = [pscustomobject]@{ X = $wr.Left; Y = $wr.Top; Width = $wr.Right - $wr.Left; Height = $wr.Bottom - $wr.Top; Name = 'Button in TrayNotifyWnd' }
  }
  if ($r -and (-not $cb -or -not (Inside $r $cb))) { return $r }
  $mine = $shown | Where-Object { $_.Name -like 'Lexpad*' } | Select-Object -First 1
  if ($mine) { Write-Host "TRAY icon on the taskbar by name at $($mine.X),$($mine.Y)"; return Rect-Of $mine }
  if (-not $cb) { Write-Host 'TRAY no icon and no overflow button'; Shot "$tag-no-overflow-button"; return $null }
  Write-Host "TRAY the icon is not on the taskbar; opening the overflow ('$($cb.Name)' at $([int]$cb.X),$([int]$cb.Y))"
  if ($cb.Name -notmatch '^Hide') {
    Click ([int]($cb.X + $cb.Width / 2)) ([int]($cb.Y + $cb.Height / 2)) 'left'
    Start-Sleep -Seconds 1
  }
  Shot "$tag-overflow-open"
  $r = Icon-Rect $tray
  if ($r -and -not (Inside $r $cb)) { return $r }
  $parts = Taskbar-Parts
  $hidden = Toolbar-Icons $parts.Hidden
  if ($parts.Hidden -ne [IntPtr]::Zero) { Say-Icons 'in the overflow' $hidden }
  $mine = $hidden | Where-Object { $_.Name -like 'Lexpad*' } | Select-Object -First 1
  if (-not $mine) {
    # Windows 11: the overflow is XAML, found from the root by name.
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $all = [System.Windows.Automation.Condition]::TrueCondition
    $icon = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $all) |
      Where-Object { $_.Current.Name -like 'Lexpad*' -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button } | Select-Object -First 1
    if ($icon) {
      $b = $icon.Current.BoundingRectangle
      $mine = [pscustomobject]@{ Name = $icon.Current.Name; X = [int]$b.X; Y = [int]$b.Y; W = [int]$b.Width; H = [int]$b.Height }
    }
  }
  if (-not $mine) {
    Write-Host 'TRAY MISSING Lexpad is in neither the taskbar''s icons nor the overflow'
    Escape
    return $null
  }
  Write-Host "TRAY HIDDEN the icon is in the overflow at $($mine.X),$($mine.Y) $($mine.W)x$($mine.H)"
  return Rect-Of $mine
}

# --- 1 and 2: right-click, the menu, Quit ---------------------------------
$run = Start-Lexpad
$r = Find-Icon $run.Tray '1'
if (-not $r) {
  Shot '1-no-icon'
  Fail 'the tray icon could not be found'
} else {
  $cx = [int](($r.Left + $r.Right) / 2); $cy = [int](($r.Top + $r.Bottom) / 2)
  Click $cx $cy 'right'
  $menu = $null
  for ($i = 0; $i -lt 20 -and -not $menu; $i++) {
    Start-Sleep -Milliseconds 250
    $menu = [TrayWin]::VisibleMenus() | Select-Object -First 1
  }
  Shot '1-right-click'
  if (-not $menu) {
    Fail 'a right-click on the icon opened no menu'
  } else {
    $hmenu = [TrayWin]::SendMessage($menu, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero) # MN_GETHMENU
    $items = [TrayWin]::Items($hmenu)
    Write-Host "TRAY menu: $(($items | Where-Object { $_ }) -join ' | ')"
    foreach ($want in 'Open Lexpad', 'Quit Lexpad') {
      if ($items -notcontains $want) { Fail "the menu has no '$want'" }
    }
    $quit = [Array]::IndexOf($items, 'Quit Lexpad')
    if ($quit -ge 0) {
      $ir = New-Object TrayWin+RECT
      [void][TrayWin]::GetMenuItemRect([IntPtr]::Zero, $hmenu, [uint32]$quit, [ref]$ir)
      Click ([int](($ir.Left + $ir.Right) / 2)) ([int](($ir.Top + $ir.Bottom) / 2)) 'left'
      if ($run.Process.WaitForExit(15000)) {
        Write-Host "TRAY Quit Lexpad: Lexpad quit (exit code $($run.Process.ExitCode))"
      } else {
        Shot '2-after-quit'
        Fail 'Quit Lexpad did not quit within 15 seconds'
      }
    } else {
      Escape
    }
  }
}

# --- 3: left-click opens the panel -----------------------------------------
# The panel is made hidden at start and shown (and focused) by the click, as
# a small window just above the icon: the window in front must be Lexpad's,
# and narrower than the Start window the launch opened.
$run = Start-Lexpad
$r = Find-Icon $run.Tray '3'
if (-not $r) {
  Shot '3-no-icon'
  Fail 'the tray icon could not be found again'
} else {
  Click ([int](($r.Left + $r.Right) / 2)) ([int](($r.Top + $r.Bottom) / 2)) 'left'
  Start-Sleep -Seconds 2
  Shot '3-left-click'
  $front = [TrayWin]::GetForegroundWindow()
  $wr = New-Object TrayWin+RECT
  [void][TrayWin]::GetWindowRect($front, [ref]$wr)
  $w = $wr.Right - $wr.Left
  Write-Host "TRAY in front after the left-click: '$([TrayWin]::TitleOf($front))' pid $([TrayWin]::PidOf($front)) at $($wr.Left),$($wr.Top) ${w}x$($wr.Bottom - $wr.Top)"
  if ([TrayWin]::PidOf($front) -ne [uint32]$run.Process.Id -or $w -ge 600) {
    Fail 'a left-click on the icon did not open the panel'
  } else {
    Write-Host 'TRAY left-click opened the panel'
  }
}
Stop-Process -Id $run.Process.Id -Force -ErrorAction SilentlyContinue

Write-Host "TRAY failures $script:failures"
if ($script:failures -gt 0) { exit 1 }
