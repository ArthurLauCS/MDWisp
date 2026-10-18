; MDWisp installer (Inno Setup 6). Build with scripts\dist.ps1, which compiles
; the release exe first and passes the version in.
; Per-user install into %LOCALAPPDATA%\Programs\MDWisp, no admin rights needed.

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId=MDWisp
AppName=MDWisp
AppVersion={#Version}
AppVerName=MDWisp {#Version}
AppPublisher=MDWisp
DefaultDirName={autopf}\MDWisp
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\target\installer
OutputBaseFilename=MDWisp-{#Version}-setup
SetupIconFile=..\crates\app\assets\app.ico
UninstallDisplayIcon={app}\mdwisp.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ShowLanguageDialog=yes
UsePreviousLanguage=no
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes

[Languages]
Name: "zh"; MessagesFile: "ChineseSimplified.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
zh.OldVersion=正在移除旧版 MDWisp（设置、会话和历史版本会保留）…
en.OldVersion=Removing the previous MDWisp (settings, sessions and history are kept)...
zh.FileType=Markdown 文档
en.FileType=Markdown document

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"

[Files]
Source: "..\target\release\mdwisp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\crates\export\assets\fonts\*"; DestDir: "{app}\fonts"; Flags: ignoreversion recursesubdirs
Source: "..\stock\*"; DestDir: "{app}\stock"; Flags: ignoreversion recursesubdirs
Source: "..\plugin\*"; DestDir: "{app}\plugin"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\MDWisp"; Filename: "{app}\mdwisp.exe"
Name: "{autodesktop}\MDWisp"; Filename: "{app}\mdwisp.exe"; Tasks: desktopicon

[Registry]
; A candidate in "Open with", never a forced default: Windows only lets the user pick that.
Root: HKCU; Subkey: "Software\Classes\MDWisp.Markdown"; ValueType: string; ValueData: "{cm:FileType}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\MDWisp.Markdown\DefaultIcon"; ValueType: string; ValueData: """{app}\mdwisp.exe"",0"
Root: HKCU; Subkey: "Software\Classes\MDWisp.Markdown\shell\open\command"; ValueType: string; ValueData: """{app}\mdwisp.exe"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "MDWisp.Markdown"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: string; ValueName: "MDWisp.Markdown"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Applications\mdwisp.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "MDWisp"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Applications\mdwisp.exe\SupportedTypes"; ValueType: string; ValueName: ".md"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\mdwisp.exe\SupportedTypes"; ValueType: string; ValueName: ".markdown"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\mdwisp.exe\shell\open\command"; ValueType: string; ValueData: """{app}\mdwisp.exe"" ""%1"""

[Run]
Filename: "{app}\mdwisp.exe"; Description: "{cm:LaunchProgram,MDWisp}"; Flags: nowait postinstall skipifsilent

[Code]
const
  { electron-builder's uninstall key for appId dev.mdview.app (MDWisp up to 0.4.x). }
  ElectronKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\1e87d5d7-1f1a-5739-a366-68891c071eb2';

{ The Electron build installed elsewhere; remove it so only one MDWisp exists.
  Its uninstaller leaves %APPDATA%\mdview alone, which the native build reads. }
procedure CurStepChanged(CurStep: TSetupStep);
var
  Cmd: String;
  Code: Integer;
begin
  if (CurStep = ssInstall) and RegQueryStringValue(HKCU, ElectronKey, 'QuietUninstallString', Cmd) then
  begin
    WizardForm.StatusLabel.Caption := CustomMessage('OldVersion');
    Exec(ExpandConstant('{cmd}'), '/C "' + Cmd + '"', '', SW_HIDE, ewWaitUntilTerminated, Code);
  end;
end;
