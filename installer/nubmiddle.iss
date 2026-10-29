; Inno Setup script for nubmiddle (Inno Setup 6.3 or later).
;
; Build (from the repository root, after `cargo build --release --target <target>`):
;   ISCC.exe /DAppVersion=0.3.0 /DArch=x64 /DExe=target\x86_64-pc-windows-msvc\release\nubmiddle.exe installer\nubmiddle.iss
; Arch is x64 or arm64. The installer is written to installer\Output\.

#ifndef AppVersion
  #error Pass the version with /DAppVersion=x.y.z
#endif
#ifndef Arch
  #error Pass the architecture with /DArch=x64 or /DArch=arm64
#endif
#ifndef Exe
  #error Pass the built exe with /DExe=path\to\nubmiddle.exe
#endif

#if Arch == "x64"
  #define ArchAllowed "x64compatible"
#elif Arch == "arm64"
  #define ArchAllowed "arm64"
#else
  #error Arch must be x64 or arm64
#endif

[Setup]
; Never change AppId: it ties upgrades and the uninstaller to earlier installs
AppId={{AB8D2E13-32BD-42F3-A583-9D2342DB7F28}
AppName=nubmiddle
AppVersion={#AppVersion}
AppVerName=nubmiddle {#AppVersion}
AppPublisher=Akiva Miura
AppPublisherURL=https://github.com/akivajp/nubmiddle
AppSupportURL=https://github.com/akivajp/nubmiddle/issues
AppUpdatesURL=https://github.com/akivajp/nubmiddle/releases
; Per-user install by default (no administrator rights); the user may choose all users instead
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\nubmiddle
DisableProgramGroupPage=yes
ArchitecturesAllowed={#ArchAllowed}
ArchitecturesInstallIn64BitMode={#ArchAllowed}
OutputDir=Output
OutputBaseFilename=nubmiddle-v{#AppVersion}-setup-{#Arch}
SetupIconFile=..\assets\nubmiddle.ico
UninstallDisplayIcon={app}\nubmiddle.exe
UninstallDisplayName=nubmiddle
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
; nubmiddle is quit with its own --quit (Restart Manager cannot close an elevated tray app)
CloseApplications=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[CustomMessages]
en.AutostartTask=Start nubmiddle with Windows, as administrator (works on elevated windows too)
ja.AutostartTask=Windows の起動時に nubmiddle を管理者として開始する (管理者権限のウィンドウでも動作します)
en.AutostartFailed=Could not set up starting with Windows. You can turn it on later from the tray menu.
ja.AutostartFailed=Windows の起動時に開始する設定ができませんでした。あとでトレイメニューから設定できます。
en.DeleteSettings=Also delete your nubmiddle settings (%1)?
ja.DeleteSettings=nubmiddle の設定 (%1) も削除しますか？

[Tasks]
; Checked on the first install only; an upgrade keeps the existing autostart as it is
Name: "autostart"; Description: "{cm:AutostartTask}"; Flags: checkedonce

[Files]
Source: "..\{#Exe}"; DestDir: "{app}"; DestName: "nubmiddle.exe"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.ja.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\nubmiddle"; Filename: "{app}\nubmiddle.exe"

[Run]
Filename: "{app}\nubmiddle.exe"; Description: "{cm:LaunchProgram,nubmiddle}"; Flags: nowait postinstall skipifsilent

[Code]
const
  TaskName = 'nubmiddle';

// "DOMAIN\user" of the user running setup, for the logon task
function CurrentUser(): String;
begin
  Result := GetEnv('USERDOMAIN');
  if Result <> '' then
    Result := Result + '\';
  Result := Result + GetUserNameString();
end;

// Quits a running nubmiddle (wherever it was started from) with the given exe's --quit
procedure QuitNubmiddle(Exe: String);
var
  Code: Integer;
begin
  if FileExists(Exe) then
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

// Whether the logon task exists (reading it needs no administrator rights)
function AutostartRegistered(): Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{sys}\schtasks.exe'), '/Query /TN ' + TaskName, '', SW_HIDE,
    ewWaitUntilTerminated, Code) and (Code = 0);
end;

// Before files are replaced: quit the running nubmiddle so that its exe is not locked
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  ExtractTemporaryFile('nubmiddle.exe');
  QuitNubmiddle(ExpandConstant('{tmp}\nubmiddle.exe'));
  Result := '';
end;

// After install: register the logon task through nubmiddle itself (one UAC prompt unless elevated)
procedure CurStepChanged(CurStep: TSetupStep);
var
  Code: Integer;
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('autostart') then
  begin
    if not ShellExec('runas', ExpandConstant('{app}\nubmiddle.exe'),
      '--register-autostart "' + CurrentUser() + '"', '', SW_HIDE, ewWaitUntilTerminated, Code)
      or (Code <> 0) then
      MsgBox(CustomMessage('AutostartFailed'), mbInformation, MB_OK);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
  Settings: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    QuitNubmiddle(ExpandConstant('{app}\nubmiddle.exe'));
    // Deleting the task needs administrator rights: ask only when there is one
    if AutostartRegistered() then
      ShellExec('runas', ExpandConstant('{app}\nubmiddle.exe'), '--unregister-autostart', '',
        SW_HIDE, ewWaitUntilTerminated, Code);
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    // Settings are kept unless the user says otherwise (they survive a reinstall)
    Settings := ExpandConstant('{userappdata}\nubmiddle');
    if DirExists(Settings) and not UninstallSilent() and
      (MsgBox(FmtMessage(CustomMessage('DeleteSettings'), [Settings]), mbConfirmation,
        MB_YESNO or MB_DEFBUTTON2) = IDYES) then
      DelTree(Settings, True, True, True);
  end;
end;
