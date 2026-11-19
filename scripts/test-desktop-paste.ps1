# Run in a Windows Developer PowerShell on an interactive desktop.
# The test temporarily owns foreground focus and the clipboard.
param([string]$TestExe, [string]$EdgeExe = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe")
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $EdgeExe -PathType Leaf)) { throw 'Set -EdgeExe to an installed Microsoft Edge executable.' }
if (-not $TestExe) {
    Push-Location (Split-Path $PSScriptRoot -Parent)
    try {
        $messages = cargo test --locked --release --bin vibe-voice-lite --no-run --message-format=json
        if ($LASTEXITCODE -ne 0) { throw 'Failed to build the test executable.' }
        $TestExe = $messages | ForEach-Object { ConvertFrom-Json -InputObject $_ } |
            Where-Object { $_.target.name -eq 'vibe-voice-lite' -and $_.executable } |
            Select-Object -Last 1 -ExpandProperty executable
    } finally {
        Pop-Location
    }
}
if (-not $TestExe -or -not (Test-Path -LiteralPath $TestExe -PathType Leaf)) {
    throw 'Could not locate the vibe-voice-lite test executable.'
}
Add-Type -AssemblyName System.Windows.Forms
if (-not ('FixtureDesktop' -as [type])) {
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class FixtureDesktop {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr w);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr w,StringBuilder b,int n);
 public static string Title(IntPtr window) {
     var caption = new StringBuilder(512);
     GetWindowText(window, caption, caption.Capacity);
     return caption.ToString();
 }
 public delegate bool EnumProc(IntPtr window, IntPtr param);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr param);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
 public static IntPtr Find(string prefix) {
     IntPtr found = IntPtr.Zero;
     EnumWindows((window, param) => {
         if (!IsWindowVisible(window) || !Title(window).StartsWith(prefix)) return true;
         found = window;
         return false;
     }, IntPtr.Zero);
     return found;
 }

 // Preserve native formats without serializing any existing clipboard content to disk.
 [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr owner);
 [DllImport("user32.dll")] static extern bool CloseClipboard();
 [DllImport("user32.dll")] static extern bool EmptyClipboard();
 [DllImport("user32.dll")] static extern uint EnumClipboardFormats(uint format);
 [DllImport("user32.dll")] static extern IntPtr GetClipboardData(uint format);
 [DllImport("user32.dll")] static extern IntPtr SetClipboardData(uint format, IntPtr data);
 [DllImport("ole32.dll")] static extern IntPtr OleDuplicateData(IntPtr data, uint format, uint flags);
 [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
 static Dictionary<uint,IntPtr> saved = new Dictionary<uint,IntPtr>();
 public static void SaveClipboard(IntPtr owner) {
     if (!OpenClipboard(owner)) throw new Exception("Clipboard busy.");
     saved.Clear();
     try {
         uint format = 0;
         while ((format = EnumClipboardFormats(format)) != 0) {
             var copy = OleDuplicateData(GetClipboardData(format), format, 0);
             if (copy == IntPtr.Zero) throw new Exception("Cannot preserve a clipboard format.");
             saved.Add(format, copy);
         }
     } finally {
         CloseClipboard();
     }
 }
 public static void RestoreClipboard(IntPtr owner, uint testSequence) {
     if (!OpenClipboard(owner)) throw new Exception("Clipboard busy.");
     try {
         // Leave a concurrent clipboard change made by the user alone.
         if (GetClipboardSequenceNumber() != testSequence) return;
         if (!EmptyClipboard()) throw new Exception("Could not empty test clipboard.");
         foreach (var format in saved) {
             if (SetClipboardData(format.Key, format.Value) == IntPtr.Zero)
                 throw new Exception("Could not restore a clipboard format.");
         }
     } finally {
         CloseClipboard();
     }
 }
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr a,IntPtr b);
}
"@
}
function Wait-Until([scriptblock]$Condition, [string]$Reason, [int]$Seconds=12) {
  $limit=[datetime]::UtcNow.AddSeconds($Seconds)
  while (-not (& $Condition)) {
    if ([datetime]::UtcNow -gt $limit) {throw "$Reason timed out"}
    [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 40
  }
}
$runId='VibePasteFixture-'+[guid]::NewGuid().ToString('N')
$tempRoot = [IO.Path]::GetTempPath()
$root = Join-Path $tempRoot $runId
[IO.Directory]::CreateDirectory($root)|Out-Null
$priorFixture = $env:VIBE_PASTE_FIXTURE
$priorConfig = $env:VIBE_CONFIG_DIR
$env:VIBE_PASTE_FIXTURE = $root
$env:VIBE_CONFIG_DIR = $root
$payload='临时测试文本 123'
$editor=[Windows.Forms.Form]::new()
$editor.Text="$runId-Edit"
$editor.Width=480
$editor.Height=200
$edit=[Windows.Forms.TextBox]::new()
$edit.Multiline=$true
$edit.Dock='Top'
$other=[Windows.Forms.TextBox]::new()
$other.Dock='Bottom'
$editor.Controls.Add($edit)
$editor.Controls.Add($other)
$previousWindow=[FixtureDesktop]::GetForegroundWindow()
$edge=[IntPtr]::Zero
$keeper=[uint32]0
function Run-Case([bool]$Changed,[scriptblock]$Switch) {
  foreach($file in @('ready','send','expect-switch','clipboard-sequence')) {
    $path=Join-Path $root $file
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
  }
  if($Changed){[IO.File]::WriteAllText((Join-Path $root 'expect-switch'),'')}
  $p=Start-Process -FilePath $TestExe -ArgumentList @('services::tests::desktop_paste_fixture','--exact','--ignored','--nocapture') -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $root 'out.log') -RedirectStandardError (Join-Path $root 'err.log')
  Wait-Until {([IO.File]::Exists((Join-Path $root 'ready'))) -or $p.HasExited} 'resolve fixture target'
  if($p.HasExited){throw (Get-Content (Join-Path $root 'err.log') -Raw)}
  & $Switch
  [IO.File]::WriteAllText((Join-Path $root 'send'),'')
  Wait-Until {$p.HasExited} 'delivery'
  $seqpath=Join-Path $root 'clipboard-sequence'
  if([IO.File]::Exists($seqpath)){$script:keeper=[uint32]([IO.File]::ReadAllText($seqpath))}
  if($p.ExitCode -ne 0){throw (Get-Content (Join-Path $root 'err.log') -Raw)}
}
try{
  [FixtureDesktop]::SaveClipboard($editor.Handle)
  $editor.Show();$editor.Activate();$edit.Focus()|Out-Null
  Wait-Until {[FixtureDesktop]::GetForegroundWindow() -eq $editor.Handle -and $edit.Focused} 'focus native editor'
  Run-Case $false {}
  Wait-Until {$edit.Text -ceq $payload} 'text in native editor'
  Write-Output 'PASS native editor received exact UTF-16 result'
  Run-Case $true {$other.Focus()|Out-Null;[Windows.Forms.Application]::DoEvents()}
  if ($other.Text.Length -ne 0) {throw 'paste sent to switched input'}
  Write-Output 'PASS switching native text boxes suppressed paste'
  foreach($tab in @('A','B')){
    $field = if ($tab -eq 'A') { '<textarea id="field" autofocus></textarea>' } else { '<div id="field" role="textbox" contenteditable="true" autofocus></div>' }
    $html="<!doctype html><meta charset='utf-8'><title>$runId-$tab</title>$field<script>const e=document.getElementById('field');e.oninput=()=>{if((e.isContentEditable?e.textContent:e.value)==='$payload')document.title='$runId-$tab-OK'};onfocus=()=>e.focus();e.focus();</script>"
    [IO.File]::WriteAllText((Join-Path $root "$tab.html"),$html)
  }
  $profile=Join-Path $root 'edge-profile'
  $a=([uri](Join-Path $root 'A.html')).AbsoluteUri
  $b=([uri](Join-Path $root 'B.html')).AbsoluteUri
  Run-Case $true {
    Start-Process -FilePath $EdgeExe -ArgumentList @(("--user-data-dir=`"{0}`"" -f $profile),'--no-first-run','--no-default-browser-check','--disable-sync','--disable-background-networking','--new-window',$a,$b) -WindowStyle Normal
    Wait-Until {
      $script:edge = [FixtureDesktop]::Find("$runId-A")
      if ($script:edge -eq [IntPtr]::Zero) { $script:edge = [FixtureDesktop]::Find("$runId-B") }
      $script:edge -ne [IntPtr]::Zero
    } 'fixture browser window' 18
    [FixtureDesktop]::SetForegroundWindow($edge) | Out-Null
    Wait-Until { [FixtureDesktop]::GetForegroundWindow() -eq $edge } 'focus browser test profile'
  }
  if ([FixtureDesktop]::Title($edge).Contains('-OK')) { throw 'paste sent to another application' }
  Write-Output 'PASS switching from native editor to Edge suppressed paste'
  Run-Case $false {}
  Wait-Until {[FixtureDesktop]::Title($edge).Contains('-OK')} 'matching text in first browser field'
  $startTitle=[FixtureDesktop]::Title($edge)
  Run-Case $true {
    if ([FixtureDesktop]::GetForegroundWindow() -ne $edge) {throw 'fixture browser lost focus'}
    [Windows.Forms.SendKeys]::SendWait('^{TAB}')
    Wait-Until {([FixtureDesktop]::Title($edge) -ne $startTitle) -and [FixtureDesktop]::Title($edge).StartsWith($runId)} 'select second local tab'
  }
  if([FixtureDesktop]::Title($edge).Contains('-OK')){throw 'second browser tab received unexpected result'}
  Write-Output 'PASS switching Edge tabs suppressed paste'
  Run-Case $false {}
  Wait-Until {[FixtureDesktop]::Title($edge).Contains('-OK')} 'matching text in second browser field'
  Write-Output 'PASS Edge textarea and contenteditable chat field each received exact UTF-16 result'
}finally{
  if($edge -ne [IntPtr]::Zero -and [FixtureDesktop]::Title($edge).StartsWith($runId)){
    [FixtureDesktop]::PostMessage($edge,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null
  }
  if ($keeper -ne 0){[FixtureDesktop]::RestoreClipboard($editor.Handle,$keeper)}
  $editor.Close()
  $editor.Dispose()
  if ($previousWindow -ne [IntPtr]::Zero) { [FixtureDesktop]::SetForegroundWindow($previousWindow) | Out-Null }
  $env:VIBE_PASTE_FIXTURE = $priorFixture
  $env:VIBE_CONFIG_DIR = $priorConfig
  # Remove only the unique fixture created above. Edge may briefly hold its profile open.
  $resolved = (Resolve-Path -LiteralPath $root).Path
  if ($resolved -eq [IO.Path]::GetFullPath((Join-Path $tempRoot $runId))) {
      for ($attempt = 0; $attempt -lt 5; $attempt++) {
          try {
              Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction Stop
              break
          } catch {
              Start-Sleep -Milliseconds 400
              if ($attempt -eq 4) { Write-Warning "Temporary fixture still in use: $resolved" }
          }
      }
  }
}
