# CI's check of the Windows icon, on the runner only, after the NSIS
# installer's silent install.
#
# 1. The exe's icon group (RT_GROUP_ICON): every size the shell asks for is
#    there (16, 24, 32, 48, 64 and 256 at least), each with planes 1 and
#    32 bits, the small ones as BMP and 256 as PNG (scripts/icons.py).
# 2. The shortcuts the installer made (desktop and Start menu): each points
#    at the installed exe and takes its icon from it.
# 3. What Explorer draws: the shell renders the exe and each shortcut at 16,
#    32, 48, 72, 96 and 256 px (IShellItemImageFactory, as the desktop and
#    Start do), and each picture must be mostly the mark's green, not the
#    blank white icon Windows draws when it cannot read one. The pictures
#    are written to icon-check/ for the artifact.
param([Parameter(Mandatory)] [string]$Exe)
$ErrorActionPreference = 'Stop'

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

public static class IconProbe {
  [DllImport("kernel32", SetLastError = true, CharSet = CharSet.Unicode)]
  static extern IntPtr LoadLibraryEx(string file, IntPtr reserved, uint flags);
  [DllImport("kernel32")] static extern bool FreeLibrary(IntPtr module);
  delegate bool EnumNameProc(IntPtr module, IntPtr type, IntPtr name, IntPtr param);
  [DllImport("kernel32", CharSet = CharSet.Unicode)]
  static extern bool EnumResourceNames(IntPtr module, IntPtr type, EnumNameProc proc, IntPtr param);
  [DllImport("kernel32")] static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
  [DllImport("kernel32")] static extern IntPtr LoadResource(IntPtr module, IntPtr res);
  [DllImport("kernel32")] static extern IntPtr LockResource(IntPtr data);
  [DllImport("kernel32")] static extern uint SizeofResource(IntPtr module, IntPtr res);

  static byte[] Read(IntPtr module, IntPtr name, int type) {
    IntPtr res = FindResource(module, name, (IntPtr)type);
    if (res == IntPtr.Zero) return null;
    uint size = SizeofResource(module, res);
    IntPtr ptr = LockResource(LoadResource(module, res));
    byte[] bytes = new byte[size];
    Marshal.Copy(ptr, bytes, 0, (int)size);
    return bytes;
  }

  // "size planes bits format" for each entry of the first icon group.
  public static string[] Group(string exe) {
    IntPtr module = LoadLibraryEx(exe, IntPtr.Zero, 0x2 | 0x20);
    if (module == IntPtr.Zero) throw new Exception("cannot load " + exe);
    try {
      IntPtr first = IntPtr.Zero;
      EnumResourceNames(module, (IntPtr)14, (m, t, n, p) => { first = n; return false; }, IntPtr.Zero);
      if (first == IntPtr.Zero) throw new Exception("no icon group in " + exe);
      byte[] dir = Read(module, first, 14);
      int count = BitConverter.ToUInt16(dir, 4);
      var list = new List<string>();
      for (int i = 0; i < count; i++) {
        int o = 6 + 14 * i;
        int w = dir[o] == 0 ? 256 : dir[o];
        int planes = BitConverter.ToUInt16(dir, o + 4);
        int bits = BitConverter.ToUInt16(dir, o + 6);
        int id = BitConverter.ToUInt16(dir, o + 12);
        byte[] icon = Read(module, (IntPtr)id, 3);
        string format = icon == null ? "missing" : (icon[0] == 0x89 && icon[1] == 0x50 ? "PNG" : "BMP");
        list.Add(w + " " + planes + " " + bits + " " + format);
      }
      return list.ToArray();
    } finally { FreeLibrary(module); }
  }

  [StructLayout(LayoutKind.Sequential)] struct SIZE { public int cx; public int cy; }
  [ComImport, Guid("bcc18b79-ba16-442f-80c4-8a59c30c463b"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IShellItemImageFactory { [PreserveSig] int GetImage(SIZE size, int flags, out IntPtr bitmap); }
  [DllImport("shell32", CharSet = CharSet.Unicode, PreserveSig = false)]
  static extern void SHCreateItemFromParsingName(string path, IntPtr bc,
    [MarshalAs(UnmanagedType.LPStruct)] Guid riid, [MarshalAs(UnmanagedType.Interface)] out IShellItemImageFactory item);
  [DllImport("gdi32")] static extern bool DeleteObject(IntPtr obj);

  // The shell's picture of a file's icon at `px`, saved to `png`; returns the
  // share of its pixels that are the mark's green.
  public static double Render(string path, int px, string png) {
    IShellItemImageFactory item;
    SHCreateItemFromParsingName(path, IntPtr.Zero, typeof(IShellItemImageFactory).GUID, out item);
    IntPtr hbitmap;
    int hr = item.GetImage(new SIZE { cx = px, cy = px }, 0x4 /* SIIGBF_ICONONLY */, out hbitmap);
    if (hr != 0) throw new Exception("GetImage " + path + " " + px + ": 0x" + hr.ToString("x"));
    try {
      using (Bitmap bmp = Image.FromHbitmap(hbitmap)) {
        bmp.Save(png, ImageFormat.Png);
        int green = 0;
        for (int y = 0; y < bmp.Height; y++)
          for (int x = 0; x < bmp.Width; x++) {
            Color c = bmp.GetPixel(x, y);
            if (c.G > 80 && c.G > c.R + 40 && c.G > c.B + 20) green++;
          }
        return (double)green / (bmp.Width * bmp.Height);
      }
    } finally { DeleteObject(hbitmap); }
  }
}
'@

$failures = 0
function Fail([string]$message) { Write-Host "::error::$message"; $script:failures++ }

# 1. The icon group.
Write-Host "Icon group of $Exe (size planes bits format):"
$entries = [IconProbe]::Group($Exe)
$entries | ForEach-Object { Write-Host "  $_" }
$sizes = @{}
foreach ($e in $entries) {
  $size, $planes, $bits, $format = $e -split ' '
  $sizes[[int]$size] = $true
  if ($planes -ne '1' -or $bits -ne '32') { Fail "icon $size px: planes $planes, bits $bits (want 1 and 32)" }
  if ($format -eq 'missing') { Fail "icon $size px: no RT_ICON" }
  if ([int]$size -lt 256 -and $format -ne 'BMP') { Fail "icon $size px is $format (want BMP)" }
  if ([int]$size -eq 256 -and $format -ne 'PNG') { Fail "icon 256 px is $format (want PNG)" }
}
foreach ($want in 16, 24, 32, 48, 64, 256) { if (-not $sizes[$want]) { Fail "icon group has no $want px layer" } }

# 2. The shortcuts.
$shell = New-Object -ComObject WScript.Shell
$links = @(
  Join-Path ([Environment]::GetFolderPath('Desktop')) 'Lexpad.lnk'
  Get-ChildItem (Join-Path ([Environment]::GetFolderPath('Programs')) '*') -Recurse -Filter 'Lexpad.lnk' | ForEach-Object FullName
) | Where-Object { $_ } | Select-Object -Unique
$desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Lexpad.lnk'
if (-not (Test-Path $desktop)) { Fail "no desktop shortcut at $desktop" }
$exeFull = (Resolve-Path $Exe).Path
foreach ($lnk in $links) {
  if (-not (Test-Path $lnk)) { continue }
  $s = $shell.CreateShortcut($lnk)
  Write-Host "Shortcut $lnk"
  Write-Host "  target $($s.TargetPath)"
  Write-Host "  icon   $($s.IconLocation)"
  if ($s.TargetPath -ne $exeFull) { Fail "$lnk points at $($s.TargetPath), not $exeFull" }
  # Empty (",0") means the target's own first icon; otherwise it must be the exe's.
  $iconFile = ($s.IconLocation -split ',')[0]
  if ($iconFile -and $iconFile -ne $exeFull) { Fail "$lnk takes its icon from $iconFile" }
}

# 3. What Explorer draws.
$out = Join-Path (Get-Location) 'icon-check'
New-Item -ItemType Directory -Force $out | Out-Null
$targets = @(@{ Name = 'exe'; Path = $exeFull }) + @($links | Where-Object { Test-Path $_ } | ForEach-Object {
  @{ Name = ('lnk-' + ((Split-Path (Split-Path $_ -Parent) -Leaf) -replace '[^A-Za-z0-9]', '')); Path = $_ } })
foreach ($t in $targets) {
  foreach ($px in 16, 32, 48, 72, 96, 256) {
    $png = Join-Path $out "$($t.Name)-$px.png"
    $share = [IconProbe]::Render($t.Path, $px, $png)
    Write-Host ('{0,-24} {1,4} px  green {2:P0}' -f $t.Name, $px, $share)
    if ($share -lt 0.3) { Fail "$($t.Path) at $px px is not the Lexpad mark (green $([math]::Round($share * 100)) %)" }
  }
}

if ($failures -gt 0) { throw "$failures icon check(s) failed" }
Write-Host 'Icon check passed'
