; Inno Setup script for me-voice. Build it with:  .\scripts\build.ps1 -Installer
; Output: installer\output\me-voice-setup-<version>.exe
;
; Installs per user (no admin rights) into %LOCALAPPDATA%\Programs\me-voice.
; The speech model is not included; the app offers to download it on first start.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F2B8E4C-3D1A-4B7E-9C55-2A8F0E61D7B3}
AppName=me-voice
AppVersion={#AppVersion}
AppVerName=me-voice {#AppVersion}
AppPublisher=Jeroen Hof
DefaultDirName={localappdata}\Programs\me-voice
DefaultGroupName=me-voice
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=output
OutputBaseFilename=me-voice-setup-{#AppVersion}
SetupIconFile=build\me-voice.ico
UninstallDisplayIcon={app}\me-voice.ico
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=force
; Same AppId as the old mc-voice installer, so this replaces it. Don't reuse its folder.
UsePreviousAppDir=no
UsePreviousGroup=no

[Tasks]
Name: "startup"; Description: "Start me-voice when I sign in to Windows"; GroupDescription: "Options:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Options:"; Flags: unchecked

[Files]
Source: "..\target\release\me-voice.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "build\me-voice.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\me-voice"; Filename: "{app}\me-voice.exe"; IconFilename: "{app}\me-voice.ico"
Name: "{group}\me-voice settings"; Filename: "{app}\me-voice.exe"; Parameters: "settings"; IconFilename: "{app}\me-voice.ico"
Name: "{autodesktop}\me-voice"; Filename: "{app}\me-voice.exe"; IconFilename: "{app}\me-voice.ico"; Tasks: desktopicon

[Registry]
; Same value the "Start at login" setting in the app uses, so both stay in sync.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "me-voice"; ValueData: """{app}\me-voice.exe"""; Flags: uninsdeletevalue; Tasks: startup
; The app used to be called mc-voice: remove its start-at-login entry.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "mc-voice"; Flags: deletevalue

[InstallDelete]
; Remove the old mc-voice install and shortcuts. Its settings and models are moved by the app itself.
Type: filesandordirs; Name: "{localappdata}\Programs\mc-voice"
Type: filesandordirs; Name: "{userprograms}\mc-voice"
Type: files; Name: "{userdesktop}\mc-voice.lnk"

[Run]
Filename: "{app}\me-voice.exe"; Description: "Start me-voice now"; Flags: nowait postinstall skipifsilent
; Automatic updates install silently: start the new version again.
Filename: "{app}\me-voice.exe"; Flags: nowait; Check: WizardSilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM me-voice.exe"; Flags: runhidden; RunOnceId: "StopApp"

[UninstallDelete]
; Settings and downloaded models are kept, so a reinstall doesn't re-download the model.
; Delete them by hand from %APPDATA%\me-voice and %LOCALAPPDATA%\me-voice if you want.
Type: dirifempty; Name: "{app}"

[Code]
// Stop a running mc-voice (the old name) so its folders can be moved and its files removed.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM mc-voice.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := '';
end;
